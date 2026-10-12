-- Phase 9: Align Supabase schema with SQL queries in commands.rs
--
-- Migration 5 (threads_polls) already added most columns needed by the code:
--   - threads has created_at, updated_at, applied_tag_ids, archived, locked, pinned
--   - polls has question, allow_multiselect, results_finalized
--   - poll_options table exists (no view alias needed)
--   - pinned_messages table exists
--
-- Remaining mismatches that still need fixing:
--   - messages has `message_id bigint` (snowflake) but code queries `discord_id`
--   - messages lacks `discord_reply_to bigint` for reply tracking
--   - channels has `channel_id` but not `discord_channel_id`/`discord_guild_id`
--   - public.users lacks `is_bot`
--   - user_settings lacks `last_read_message_id`/`last_read_at`
--   - guild_memberships lacks `discord_user_id`/`discord_guild_id`/`guild_roles`
--   - typing_indicators (from migration 0) uses UUID cols but code inserts bigint
--   - threads lacks `deleted_at`
--   - role_permissions table is missing
--   - servers table is missing (queried in check_user_permissions)
--   - dm_participants table is missing
-- ============================================================================

-- 1. messages: add discord_id column (mirrors message_id), add discord_reply_to
alter table messages add column if not exists discord_id bigint;
update messages set discord_id = message_id;
alter table messages alter column discord_id set not null;

alter table messages add column if not exists discord_reply_to bigint;

create unique index if not exists idx_messages_discord_id on messages(discord_id);
create index if not exists idx_messages_reply_to on messages(discord_reply_to);

-- 2. channels: add discord_channel_id and discord_guild_id snowflake aliases
alter table channels add column if not exists discord_channel_id bigint;
alter table channels add column if not exists discord_guild_id bigint;

update channels set discord_channel_id = channel_id;
update channels c
set discord_guild_id = g.guild_id
from guilds g
where c.guild_id = g.id;

alter table channels alter column discord_channel_id set not null;
create unique index if not exists idx_channels_discord_channel_id
    on channels(discord_channel_id);
create index if not exists idx_channels_discord_guild_id
    on channels(discord_guild_id);

-- 3. public.users: add is_bot
alter table public.users add column if not exists is_bot boolean default false;

-- 4. user_settings: add last_read_message_id, last_read_at
alter table user_settings add column if not exists last_read_message_id bigint;
alter table user_settings add column if not exists last_read_at timestamptz;

-- 5. guild_memberships: add discord_user_id, discord_guild_id, guild_roles
alter table guild_memberships add column if not exists discord_user_id bigint;
alter table guild_memberships add column if not exists discord_guild_id bigint;
alter table guild_memberships add column if not exists guild_roles bigint[] default '{}';

update guild_memberships gm
set discord_user_id = u.discord_id
from public.users u
where gm.user_id = u.id;

update guild_memberships gm
set discord_guild_id = g.guild_id
from guilds g
where gm.guild_id = g.id;

alter table guild_memberships alter column discord_user_id set not null;
alter table guild_memberships alter column discord_guild_id set not null;
create index if not exists idx_guild_memberships_user_guild
    on guild_memberships (discord_user_id, discord_guild_id);

-- 6. threads: add deleted_at (already has created_at, updated_at, applied_tag_ids)
alter table threads add column if not exists deleted_at timestamptz;

-- 7. role_permissions table (queried in check_user_permissions)
create table if not exists role_permissions (
    role_id    bigint not null,
    permission text not null,
    primary key (role_id, permission)
);

-- 8. servers table (queried in check_user_permissions for guild owner check)
create table if not exists servers (
    id               uuid primary key default gen_random_uuid(),
    discord_guild_id bigint not null unique,
    owner_id         bigint not null,
    created_at       timestamptz not null default now()
);

-- 9. dm_participants table
create table if not exists dm_participants (
    channel_id      bigint not null,
    discord_user_id bigint not null,
    primary key (channel_id, discord_user_id)
);

-- 10. typing_indicators: drop and recreate with discord_* bigint columns
--     Migration 0 created it with UUID columns (channel_id, user_id, started_at)
--     but the code inserts discord_channel_id / discord_user_id bigints.
drop table if exists typing_indicators;
create table typing_indicators (
    discord_channel_id bigint not null,
    discord_user_id    bigint not null,
    expires_at         timestamptz not null,
    primary key (discord_channel_id, discord_user_id)
);
alter table typing_indicators enable row level security;

create policy "typing visible in accessible channels"
    on typing_indicators for select
    using (
        discord_channel_id in (
            select c.discord_channel_id from channels c
            where (c.guild_id is not null and c.discord_guild_id in (
                select g.guild_id from guilds g
                join guild_memberships gm on gm.discord_guild_id = g.guild_id
                where gm.discord_user_id = (select discord_id from public.users where auth_user_id = auth.uid())
            ))
            or (c.guild_id is null and c.channel_id in (
                select dm.channel_id from dm_participants dm
                where dm.discord_user_id = (select discord_id from public.users where auth_user_id = auth.uid())
            ))
        )
    );
