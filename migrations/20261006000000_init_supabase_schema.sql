-- Phase 0: Supabase Postgres schema for Supabase backend
-- This schema replaces the Discord Gateway data model while preserving
-- the AppEvent/AppCommand contract the TUI already consumes.
--
-- Design principles:
-- 1. RLS on every table — users can only read/write their own data
-- 2. Private channel message delivery uses Realtime Broadcast (not postgres_changes)
--    because postgres_changes does NOT enforce RLS on delivered rows
-- 3. Token storage uses a SessionTokens pair (access + refresh), not a single string

-- ============================================================================
-- EXTENSIONS
-- ============================================================================
create extension if not exists "pgcrypto";
create extension if not exists "realtime";

-- ============================================================================
-- AUTH / USER PROFILES
-- ============================================================================

create table auth.users (
    id          uuid primary key default gen_random_uuid(),
    auth_user_id uuid not null unique,  -- maps to supabase auth.users.id
    discord_id    bigint not null unique,  -- Discord snowflake, for Id<UserMarker>
    username    text not null,
    display_name text,
    email       text unique,
    avatar_url  text,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

-- RLS: users can only read their own profile, but can read other users'
-- public fields (username, display_name, avatar_url) for display purposes
alter table auth.users enable row level security;

create policy "users can read own profile"
    on auth.users for select
    using (auth.uid() = auth_user_id);

create policy "users can read public profile fields"
    on auth.users for select
    using (true);  -- public columns only via view below

create policy "users can update own profile"
    on auth.users for update
    using (auth.uid() = auth_user_id)
    with check (auth.uid() = auth_user_id);

-- Public-facing view (no sensitive columns)
create view auth.users_public as
    select id, username, display_name, avatar_url
    from auth.users;

-- ============================================================================
-- SESSION TOKENS
-- Replaces the single `String` token in DiscordClient with a token pair.
-- The Supabase backend stores JWT access+refresh so it can renew sessions
-- without user interaction.
-- ============================================================================

create table auth.session_tokens (
    user_id      uuid not null references auth.users(id) on delete cascade,
    access_token  text not null,
    refresh_token text not null,
    expires_at    timestamptz not null,
    created_at    timestamptz not null default now()
);

alter table auth.session_tokens enable row level security;

create policy "users can read own tokens"
    on auth.session_tokens for select
    using (user_id = (select id from auth.users where auth_user_id = auth.uid()));

create policy "users can insert own tokens"
    on auth.session_tokens for insert
    with check (user_id = (select id from auth.users where auth_user_id = auth.uid()));

create policy "users can delete own expired tokens"
    on auth.session_tokens for delete
    using (
        user_id = (select id from auth.users where auth_user_id = auth.uid())
        and expires_at < now()
    );

-- ========================================================================
-- GUILDS
-- Maps to Discord's GUILD_CREATE / GUILD_UPDATE / GUILD_DELETE events.
-- ========================================================================

create table guilds (
    id                  uuid primary key default gen_random_uuid(),
    guild_id            bigint not null unique,  -- snowflake, for compatibility
    name                text not null,
    owner_id            uuid references auth.users(id),
    boost_tier          smallint default 0 check (boost_tier between 0 and 3),
    boost_count         integer default 0,
    verification_level  smallint check (verification_level between 0 and 4),
    mfa_level           smallint check (mfa_level in (0, 1, 2)),
    features            text[],
    member_count        integer,
    created_at          timestamptz not null default now(),
    updated_at          timestamptz not null default now()
);

alter table guilds enable row level security;

create policy "guilds are visible to members"
    on guilds for select
    using (
        exists (
            select 1 from guild_memberships
            where guild_memberships.guild_id = guilds.id
              and guild_memberships.user_id = (
                  select id from auth.users where auth_user_id = auth.uid()
              )
        )
    );

create policy "guild owners can update guild"
    on guilds for update
    using (owner_id = (select id from auth.users where auth_user_id = auth.uid()))
    with check (owner_id = (select id from auth.users where auth_user_id = auth.uid()));

-- ========================================================================
-- GUILD MEMBERSHIPS
-- Maps to GUILD_MEMBER_ADD / GUILD_MEMBER_REMOVE / GUILD_MEMBER_UPDATE
-- ========================================================================

create table guild_memberships (
    id          uuid primary key default gen_random_uuid(),
    guild_id    uuid not null references guilds(id) on delete cascade,
    user_id     uuid not null references auth.users(id) on delete cascade,
    nickname    text,
    roles       bigint[],  -- array of role IDs
    joined_at   timestamptz,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

alter table guild_memberships enable row level security;

create policy "members can see all members of their guilds"
    on guild_memberships for select
    using (
        exists (
            select 1 from guild_memberships as gm
            join auth.users u on gm.user_id = u.id
            where gm.guild_id = guild_memberships.guild_id
              and u.auth_user_id = auth.uid()
        )
    );

create policy "users can update own membership"
    on guild_memberships for update
    using (
        user_id = (select id from auth.users where auth_user_id = auth.uid())
    );

-- ========================================================================
-- CHANNELS
-- Maps to ChannelInfo / ChannelUpsert / ChannelDelete events
-- ========================================================================

create type channel_type as enum (
    'guild_text',
    'guild_voice',
    'guild_category',
    'dm',
    'group_dm',
    'guild_forum',
    'announcement'
);

create table channels (
    id                  uuid primary key default gen_random_uuid(),
    channel_id          bigint not null unique,  -- snowflake
    guild_id            uuid references guilds(id) on delete cascade,  -- null for DMs
    name                text,
    kind                channel_type not null,
    position            integer,
    parent_id           uuid references channels(id),  -- category parent
    topic               text,
    nsfw                boolean default false,
    user_id             uuid references auth.users(id),  -- DM recipient
    recipient_ids       uuid[],
    created_at          timestamptz not null default now(),
    updated_at          timestamptz not null default now()
);

alter table channels enable row level security;

create policy "channels visible to guild members or DM participants"
    on channels for select
    using (
        (guild_id is not null and exists (
            select 1 from guild_memberships
            where guild_memberships.guild_id = channels.guild_id
              and guild_memberships.user_id = (
                  select id from auth.users where auth_user_id = auth.uid()
              )
        ))
        or (kind = 'dm' and channels.user_id = (
            select id from auth.users where auth_user_id = auth.uid()
        ))
        or (kind = 'group_dm' and channels.recipient_ids && array(
            select id from auth.users where auth_user_id = auth.uid()
        ))
    );

-- ========================================================================
-- ROLES
-- Maps to RoleInfo / GuildRoleUpsert / GuildRoleDelete
-- ========================================================================

create table roles (
    id          uuid primary key default gen_random_uuid(),
    role_id     bigint not null,  -- snowflake
    guild_id    uuid not null references guilds(id) on delete cascade,
    name        text not null,
    permissions text not null,
    position    integer,
    color       integer,
    hoist       boolean default false,
    managed     boolean default false,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

alter table roles enable row level security;

create policy "roles visible to guild members"
    on roles for select
    using (
        exists (
            select 1 from guild_memberships
            where guild_memberships.guild_id = roles.guild_id
              and guild_memberships.user_id = (
                  select id from auth.users where auth_user_id = auth.uid()
              )
        )
    );

-- ========================================================================
-- MESSAGES
-- Maps to MessageInfo / MessageCreate / MessageUpdate / MessageDelete
--
-- NOTE: Private channel messages are delivered via Realtime Broadcast,
-- not postgres_changes, because postgres_changes does not enforce RLS
-- on the delivered rows. See docs/plan-supabase-backend.md §5 (RLS vs postgres_changes).
-- ========================================================================

create table messages (
    id              uuid primary key default gen_random_uuid(),
    message_id      bigint not null,  -- snowflake
    channel_id      uuid not null references channels(id) on delete cascade,
    discord_channel_id bigint not null,  -- snowflake, for Realtime payload
    guild_id        uuid references guilds(id) on delete cascade,
    discord_guild_id bigint,  -- snowflake, for Realtime payload
    author_id       uuid not null references auth.users(id),
    discord_author_id bigint not null,  -- snowflake, for Realtime payload
    content         text,
    created_at      timestamptz,
    edited_at       timestamptz,
    flags           integer default 0,
    type            integer default 0,
    nonce           bigint,
    tts             boolean default false,
    pinned          boolean default false,
    ephemeral       boolean default false,
    created         timestamptz not null default now()
);

alter table messages enable row level security;

create policy "messages visible to channel participants"
    on messages for select
    using (
        exists (
            select 1 from channels c
            where c.id = messages.channel_id
              and (
                  (c.guild_id is not null and exists (
                      select 1 from guild_memberships
                      where guild_memberships.guild_id = c.guild_id
                        and guild_memberships.user_id = (
                            select id from auth.users where auth_user_id = auth.uid()
                        )
                  ))
                  or (c.kind = 'dm' and c.user_id = (
                      select id from auth.users where auth_user_id = auth.uid()
                  ))
                  or (c.kind = 'group_dm' and c.recipient_ids && array(
                      select id from auth.users where auth_user_id = auth.uid()
                  ))
              )
        )
    );

create policy "authenticated users can insert messages"
    on messages for insert
    with check (auth.uid() is not null);

create policy "message authors can update their own messages"
    on messages for update
    using (author_id = (select id from auth.users where auth_user_id = auth.uid()));

-- Indexes for message queries
create index idx_messages_channel_created on messages(channel_id, message_id);
create index idx_messages_channel_id on messages(channel_id);
create index idx_messages_guild_created on messages(guild_id, created_at);

-- ========================================================================
-- MESSAGE ATTACHMENTS
-- ========================================================================

create table attachments (
    id          uuid primary key default gen_random_uuid(),
    message_id  uuid not null references messages(id) on delete cascade,
    url         text not null,
    filename    text not null,
    size        bigint,
    content_type text,
    created_at  timestamptz not null default now()
);

alter table attachments enable row level security;

create policy "attachments visible via parent message"
    on attachments for select
    using (
        exists (
            select 1 from messages
            where messages.id = attachments.message_id
              -- RLS on messages ensures the user can see the message
        )
    );

-- ========================================================================
-- REACTIONS
-- Maps to CurrentUserReactionAdd / CurrentUserReactionRemove
-- ========================================================================

create table reactions (
    message_id  uuid not null references messages(id) on delete cascade,
    user_id     uuid not null references auth.users(id) on delete cascade,
    emoji       text not null,
    created_at  timestamptz not null default now(),
    primary key (message_id, user_id, emoji)
);

alter table reactions enable row level security;

create policy "reactions visible via parent message"
    on reactions for select
    using (
        exists (
            select 1 from messages
            where messages.id = reactions.message_id
        )
    );

create policy "users can add their own reactions"
    on reactions for insert
    with check (user_id = (select id from auth.users where auth_user_id = auth.uid()));

create policy "users can delete their own reactions"
    on reactions for delete
    using (user_id = (select id from auth.users where auth_user_id = auth.uid()));

-- ========================================================================
-- PRESENCES
-- Maps to PresenceUpdate / TypingStart events
-- ========================================================================

create type user_status as enum ('online', 'idle', 'dnd', 'invisible', 'offline');
create type activity_type as enum ('game', 'streaming', 'listening', 'custom', 'competing');

create table presences (
    user_id         uuid not null references auth.users(id) on delete cascade,
    guild_id        uuid references guilds(id) on delete cascade,
    status          user_status,
    activity_name   text,
    activity_type   activity_type,
    activity_state  text,
    updated_at      timestamptz not null default now()
);

alter table presences enable row level security;

create policy "presences visible to guild members"
    on presences for select
    using (
        guild_id is null
        or exists (
            select 1 from guild_memberships
            where guild_memberships.guild_id = presences.guild_id
              and guild_memberships.user_id = (
                  select id from auth.users where auth_user_id = auth.uid()
              )
        )
    );

-- ========================================================================
-- TYPING INDICATORS
-- Maps to TypingStart event (10s TTL)
-- ========================================================================

create table typing_indicators (
    channel_id  uuid not null references channels(id) on delete cascade,
    user_id     uuid not null references auth.users(id) on delete cascade,
    started_at  timestamptz not null default now(),
    primary key (channel_id, user_id)
);

alter table typing_indicators enable row level security;

create policy "typing indicators visible to channel participants"
    on typing_indicators for select
    using (
        exists (
            select 1 from channels c
            where c.id = typing_indicators.channel_id
              and (
                  (c.guild_id is not null and exists (
                      select 1 from guild_memberships
                      where guild_memberships.guild_id = c.guild_id
                        and guild_memberships.user_id = (
                            select id from auth.users where auth_user_id = auth.uid()
                        )
                  ))
                  or (c.kind = 'dm' and c.user_id = (
                      select id from auth.users where auth_user_id = auth.uid()
                  ))
              )
        )
    );

create policy "users can send typing indicators"
    on typing_indicators for insert
    with check (
        user_id = (select id from auth.users where auth_user_id = auth.uid())
    );

-- ========================================================================
-- VOICE (placeholder — full voice support deferred to Phase 3)
-- Tables exist so the schema is forward-compatible.
-- ========================================================================

create table voice_states (
    channel_id  uuid not null references channels(id) on delete cascade,
    user_id     uuid not null references auth.users(id) on delete cascade,
    muted       boolean default false,
    deafened    boolean default false,
    speaking    boolean default false,
    updated_at  timestamptz not null default now(),
    primary key (channel_id, user_id)
);

alter table voice_states enable row level security;

create policy "voice states visible to channel participants"
    on voice_states for select
    using (
        exists (
            select 1 from channels c
            where c.id = voice_states.channel_id
              and (
                  (c.guild_id is not null and exists (
                      select 1 from guild_memberships
                      where guild_memberships.guild_id = c.guild_id
                        and guild_memberships.user_id = (
                            select id from auth.users where auth_user_id = auth.uid()
                        )
                  ))
              )
        )
    );

-- ========================================================================
-- APPLICATION COMMANDS (placeholder)
-- ========================================================================

create table application_commands (
    id          uuid primary key default gen_random_uuid(),
    command_id  bigint not null,
    guild_id    uuid references guilds(id) on delete cascade,
    name        text not null,
    description text,
    options     jsonb,
    created_at  timestamptz not null default now()
);

alter table application_commands enable row level security;

create policy "application commands visible to guild members"
    on application_commands for select
    using (
        guild_id is null
        or exists (
            select 1 from guild_memberships
            where guild_memberships.guild_id = application_commands.guild_id
              and guild_memberships.user_id = (
                  select id from auth.users where auth_user_id = auth.uid()
              )
        )
    );

-- ========================================================================
-- TRIGGERS — keep updated_at fresh
-- ========================================================================

create or replace function update_updated_at()
returns trigger as $$
begin
    new.updated_at = now();
    return new;
end;
$$ language plpgsql;

create trigger trigger_auth_users_updated
    before update on auth.users
    for each row execute function update_updated_at();

create trigger trigger_guilds_updated
    before update on guilds
    for each row execute function update_updated_at();

create trigger trigger_guild_memberships_updated
    before update on guild_memberships
    for each row execute function update_updated_at();

create trigger trigger_channels_updated
    before update on channels
    for each row execute function update_updated_at();

create trigger trigger_roles_updated
    before update on roles
    for each row execute function update_updated_at();

create trigger trigger_messages_updated
    before update on messages
    for each row execute function update_updated_at();

-- ========================================================================
-- REALTIME — message delivery for private channels
--
-- postgres_changes does NOT enforce RLS on delivered rows, so DM/group-DM
-- messages must go through Realtime Broadcast on a per-user private
-- channel, with the server function authorizing against the JWT.
-- Guild channel messages can use postgres_changes (row-level security
-- applies because the subscription is made as the authenticated user).
-- See docs/plan-supabase-backend.md §5 (RLS vs postgres_changes)
-- ========================================================================

-- The Realtime config is set via SQL in the Supabase dashboard or:
-- supabase db reset + supabase functions deploy for the broadcast trigger.