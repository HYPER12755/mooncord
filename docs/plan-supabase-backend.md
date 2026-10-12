# Master plan: Discord backend → Supabase backend

Status: draft for review. All line counts and file references below were measured
directly against `_nazmiforreal` @ `5bac7a2`, not taken from prior notes.

## 0. Decisions this plan encodes

| Decision | Choice |
|---|---|
| Scope | Full Discord parity (servers, threads, forums, roles, moderation) |
| Transport | Native Rust against GoTrue / PostgREST / Realtime. No Node sidecar, no third-party crate |
| Voice | Kept, via a LiveKit SFU |
| Auth | Email OTP at signup; email OTP **or** TOTP at each login (TOTP only if the user enrolled it) |

## 1. Measured baseline

| Thing | Size |
|---|---|
| `src/` total | 214,787 lines |
| `src/discord/` | 93,268 (43%) |
| `src/tui/` | 111,680 (203 files, 38,427 of them tests) |
| `src/discord/voice/` | **35,827** |
| `src/discord/gateway/parser/` | 8,934 |
| `src/discord/rest/` | 4,561 |
| `src/discord/rpc/` | 2,054 |
| auth-shaping files (fingerprint + tests, qr_auth, password_auth, captcha) | 2,593 |

Enum surfaces that are the real contract:

- `AppCommand` — **72** variants (`src/discord/commands.rs:432`)
- `AppEvent` — **138** variants (`src/discord/events.rs:197`)
- `DiscordAction` — **24** variants (`src/discord/action_policy.rs:12`)
- keymap `Action` — **94** variants, of which ~5 are Discord-specific

### The finding that drives everything

**`grep` for `trait` across `src/app`, `src/discord`, `src/tui` returns 0.** There is no
interface boundary anywhere in the codebase. The TUI is welded to the backend:

- `DiscordUiState` (`src/tui/state/discord_ui.rs:20`) holds a `DiscordState` by value and
  `impl Deref for DiscordUiState { type Target = DiscordState }` at **:35-44**. So any
  `&DashboardState` parameter anywhere in the TUI implicitly reaches Discord state.
- `DashboardState.discord` is field 1 of 16 (`src/tui/state/dashboard.rs:12`).
- 112 of 203 `src/tui/` files import `crate::discord`; 42 non-test files taking
  `&DashboardState` account for ~20k lines of `ui/`.

## 2. The key architectural call: **do not introduce traits**

The instinct on reading "0 traits" is to define a `trait Backend` and migrate 112 files
onto it. **Don't.** The crate *already has* the seam — it's just implicit:

```
TUI ──AppCommand──▶ mpsc::Sender ──▶ command_loop ──▶ HTTP/WS
TUI ◀─SequencedAppEvent── mpsc ◀── snapshot ◀── watch::Receiver
```

`src/app/command_dispatch.rs:140` is one `match command` over the 72 variants.
`SequencedAppEvent { revision, event }` (`src/discord/events.rs:1784`) plus the
deferred-effect replay at `src/tui/runtime/effects.rs:385` (defers any effect whose
`revision > current_snapshot_revision` until the watch fires) is **transport-agnostic
and already correct**. It is a working ports-and-adapters seam that costs nothing.

So the migration is **additive, not refactoring-first**: build `src/backend/` emitting the
same `AppEvent`s and consuming the same `AppCommand`s, switch the wiring in `src/app.rs`,
then delete `src/discord/`. Adding a trait layer first would mean changing 112 file
signatures to get a boundary you already have for free.

The one thing that *does* need retyping is the read path (`DiscordState` → the
`Deref`-ed cache), and that's Phase 2 below.

## 3. Auth design (GoTrue)

All four flows map onto existing GoTrue endpoints. Concord's existing
`password_auth.rs` state machine (`Token | MfaRequired | RequiredActions`) becomes a
smaller one.

| Flow | GoTrue call |
|---|---|
| Signup | `POST /auth/v1/signup` `{email, password}` → session (send `ConfirmationType: signup`) |
| Verify signup email | `POST /auth/v1/verify` `{type: "email", email, token}` |
| Login, password only | `POST /auth/v1/token?grant_type=password` |
| Login, email OTP | `POST /auth/v1/otp` `{email}` → `POST /auth/v1/verify` `{type: "email", ...}` |
| Login, TOTP | `POST /auth/v1/factors/{id}/challenge` → `POST /auth/v1/factors/{id}/verify` |
| Check assurance | `user.app_metadata.aal` / response `aal`: `aal1` vs `aal2` |

Login flow in the client: attempt password grant → read returned `aal` → if `aal1` and
the user has an enrolled TOTP factor, show the 6-digit code screen; if the user has
enrolled email OTP and not TOTP, offer that instead. GoTrue enforces this server-side,
so the client only has to render the challenge.

**Token shape — decide this on day one.** GoTrue returns a token *pair*
(`access_token` ~1 h + `refresh_token`), where Concord has exactly one `String`
(`src/discord/client.rs:147` clones it into `DiscordRest`; attached at
`src/discord/rest.rs:131`). Retrofit a refresh cycle after the auth swap is painful.
Introduce a `SessionTokens { access, refresh, expires_at }` type before Phase 2.

Also: rename the keyring service string, currently
`io.github.chojs23.concord.discord-token.v1` (`src/support/token_store.rs:8`), so the
new tokens don't clobber a real Discord login on the same machine. The multi-account
machinery around it (`CredentialFile`, `upsert_token`, `selected_account_id`) is
backend-agnostic — keep it.

Deletions: `qr_auth.rs` (621), `captcha.rs` (59), `password_auth.rs` (420),
`fingerprint.rs` + tests (1,493 — Discord anti-fingerprinting, scrapes the build number
off discord.com), `tui/login/` (~1,090, five of six screens Discord-only). Removes `rsa`,
`sha2`, `qrcode` from `Cargo.toml`.

> Note: `src/discord/verification.rs` is **not** auth — it's guild-participation gating.
> And `aes-gcm`/`chacha20poly1305`/`davey` are voice E2EE, not auth; they survive.

## 4. Data model

`MessageInfo` (`src/discord/message.rs:645`) is 28 fields; `ChannelInfo`
(`src/discord/channel.rs:15`) carries `thread_metadata`, `rate_limit_per_user`,
`user_limit`, `owner_id`, `flags` — all Discord vocabulary that maps to real columns
under different names. Since parity is in scope, design for it up front even though chat
ships first.

`Id<T>` is a phantom-typed `NonZeroU64` (`src/discord/ids.rs:14`), i.e. Discord
snowflakes: time-sortable. **Keep that property** — it gives free chronological ordering
and cursor pagination via `range()`. Map `uuid` columns to a generated snowflake-style
`bigint` rather than `gen_random_uuid()`, or every pagination query gets a sort key.

Core tables: `profiles`, `guilds`, `guild_members`, `roles`, `channels` (kind as an enum
covering text/voice/category/forum/thread), `messages`, `message_attachments`,
`reactions`, `message_mentions`, `message_references`, `threads`, `thread_members`,
`polls` + `poll_options` + `poll_votes`, `read_states`, `webhooks`, `membership_requests`,
`bans`, `user_settings`.

Things PostgREST **cannot** do and that need Postgres RPC functions (`SECURITY DEFINER`,
`search_path` pinned):
- message search with filters + cursor
- member-list pagination / chunked hydration
- permission resolution (`has_channel_permission(uid, channel, action)`) — this is what
  replaces all 24 `DiscordAction` client-side guards
- unread counts / badge aggregation
- `send_message` as one transaction (insert + notify + fanout)

## 5. Realtime — and the RLS trap

Supabase Realtime shares **zero** protocol surface with Discord's gateway: no opcodes, no
zlib frames, no sequence numbers, no `session_id`/`resume_url`, no HELLO/heartbeat-ack
dance. Of `src/discord/gateway.rs` (2,561 lines), roughly 250–350 lines are worth
lifting: the `run_gateway` outcome loop with exponential + **full-jitter** backoff
(`gateway.rs:1012-1089`), the `GatewaySender` urgent/normal queue split, `WebSocketConfig`,
and the shared rustls platform-verifier connector (`src/support/tls.rs`). Everything
else is rewritten.

**The trap, flagged up front because it shapes the schema:** for `postgres_changes`,
Supabase does **not** enforce RLS on change delivery — a subscriber matching a filter
can receive rows it would not be permitted to `SELECT`. For a chat app that is a
data-leak vector, and it is easy to ship by accident. So:

- Do **not** back live message delivery with `postgres_changes`.
- Back it with **Realtime Broadcast on private channels**, authorized by a policy
  against the JWT (`auth.uid()` / membership), and have Postgres `notify()` the channel
  after each committed write.
- Use `postgres_changes` only for genuinely non-sensitive tables, if at all.

Resubscription backpressure: `GuildMemberRequestScheduler`
(`src/discord/gateway.rs:341-803`, ~460 lines) is a good model, but Supabase expresses it
as `phx_join` with a reissued `access_token` rather than op-8 `SUBSCRIBE_*`.

## 6. Phasing

Each phase ends with something runnable.

| # | Phase | Size | Notes |
|---|---|---|---|
| 0 | Postgres schema + migrations + RLS + seed | ~1.5k SQL | Foundation for everything. Design for parity now, ship chat first |
| 1 | GoTrue auth + `src/tui/login/` rewrite + token-pair type | ~1.2k Rust | Independently valuable; verifiable against a live Supabase project |
| 2 | `src/backend/` skeleton: PostgREST reads + cache + `AppEvent` emission | ~3k Rust | **Re-types the read path.** Highest-leverage phase. Read-only chat lands here |
| 3 | Writes via RPC: send/edit/delete/react/typing/read-state | ~2k Rust + SQL | Replaces `src/discord/rest/` route modules (4,561 lines) |
| 4 | Realtime Broadcast subscription, replacing the gateway | ~800 Rust | |
| 5 | Media → Supabase Storage; rewrite `emoji.rs` (70 lines) | ~400 Rust | Media pipeline itself is generic — only 4 CDN constants change |
| 6 | Parity: guilds, roles/permissions, threads, forums, polls, moderation | ~8k Rust + SQL | The biggest single chunk, and the reason scope is "full parity" |
| 7 | Voice → LiveKit | ~5k Rust | See §7 |
| 8 | Delete `src/discord/` (93,268 lines) | deletion | Also lets `davey`/`aes-gcm`/`chacha20poly1305` go if voice moved |

## 7. Voice reality check

`src/discord/voice/` is **35,827** lines, not the ~15k first estimated. The split:

- **~21.5k is Discord-specific and goes away**: UDP voice gateway + IP discovery
  (`voice.rs:154`), RTP payloads `0x78`/`103`/`104` (`voice.rs:160`), dAVE MLS E2EE
  (`dave.rs`, 993 lines, external `davey` crate), and above all `stream.rs` (5,443) —
  which is a hand-rolled SFU client: RTCP sender reports, jitter buffers, PLI throttling,
  transport-cc feedback, NTP clock sync, packet recovery. Plus `broadcast.rs` (3,421),
  Discord simulcast/GoLive.
- **~7.4k is screen share**, also transport-bound: PipeWire portal (1,688), X11 (871),
  EGL (610), ScreenCaptureKit (525), Windows Graphics Capture (664), and the VA-API /
  VideoToolbox / Media Foundation / openh264 encoders (3,367).
- **~6.4k is genuinely generic** and ports to any SFU: `cpal` device enumeration
  (`devices.rs`, `microphone.rs`), playback, VAD/noise suppression, resampling,
  `audio_buffer`/`levels`.
- **~1.8k** `system_audio` (PipeWire loopback, CoreAudio, WASAPI) — generic.

So keeping voice means writing a fresh transport layer against LiveKit either way; the
salvage is the audio plumbing, not the protocol. Screen share (~7.4k) is the most
expensive-to-replace, lowest-value-first item — worth revisiting after Phase 6.

## 8. What survives untouched

Roughly **48% of `src/tui/`** needs no changes: `theme.rs` (1,138), `text.rs` (720),
`fuzzy.rs` (361), `clipboard.rs` (364), `text_input.rs` (237), `text_cursor.rs` (181),
`keybindings/chord.rs` (521), `keybindings/composer.rs` (349), `media/` (8 of 9 files
have zero Discord imports — 5,798), `markdown.rs`/`wrap.rs`/`syntax_highlight.rs`,
`redraw.rs`/`redraw_gate.rs`/`scheduler.rs` (3,732 in `runtime/`), `terminal.rs`,
`selection.rs`.

Also surviving: ~9,426 lines of golden-render tests in `src/tui/ui/tests/`,
`keybindings/tests.rs` (1,498), `translation/` (LibreTranslate, 507 lines),
`klipy.rs` (357), and `request_lifecycle/` (1,288 — dedup/cancellation, transport-agnostic).

Needs retyping behind Phase 2: `state/` (28,733), `ui/` (15,483), `input/` (3,372),
`runtime/effects.rs` (783), `tui/commands.rs`.

Droppable: ~21.5k voice + 2,054 `rpc/` + ~1,050 Components V2/embeds/polls ≈ **~27,600
lines (13% of the repo)**. `src/discord/rpc/` in particular is a pure Discord-ism — it
binds `discord-ipc-N` to impersonate the desktop client for Rich Presence, with no
analogue.

## 9. Risks

1. **RLS vs `postgres_changes`** (§5) — the one that can leak data in production.
2. **Snowflake→`bigint`** — if IDs become `uuid`, every cursor query needs a tiebreaker.
3. **The read path re-type** (Phase 2) is the largest single chunk of Rust and touches
   42 files that take `&DashboardState`. Budget it as its own phase, not as part of 3.
4. **Full parity is genuinely large** — Phase 6 alone is comparable to Phases 0–5 combined.
5. **`tui/state/tests/` is 14,129 lines** of Discord-shaped fixtures; much is rewritten,
   not deleted.
6. **MSRV discrepancy in the repo today**: `Cargo.toml` says `rust-version = "1.90"`,
   `CONTRIBUTING.md:35` says MSRV is 1.85. Unrelated to this work; worth fixing.
7. **No `livekit` dependency yet** (`grep -ci livekit Cargo.toml` → 0). LiveKit's Rust SDK
   covers data channels and the SFU; audio capture still lands on the existing `cpal` path.

## 10. Open questions

1. PostgREST row limits (default 1000, max `db-max-rows`) shape message pagination.
   Acceptable, or plan around with RPCs?
2. Supabase Realtime Broadcast vs. a dedicated `pg_notify`-based path — the former is
   managed and simpler; the latter has fewer moving parts but no presence primitives.
3. Is the Supabase project self-hosted or hosted? Changes TLS/connection handling and
   whether `postgres_changes` semantics are configurable.
4. Should `Id<T>` stay `NonZeroU64`, or migrate to a `(timestamp, seq)` pair?
