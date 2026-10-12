-- Phase 6: Threads, Forums, Polls, and Moderation

-- ============================================================================
-- THREADS
-- ============================================================================

create table if not exists threads (
    id bigserial primary key,
    -- Discord snowflake for the thread channel (channel_id = thread channel id)
    channel_id bigint not null unique,
    -- Internal Discord snowflake guild_id (bigint), nullable for group DMs
    discord_guild_id bigint,
    -- Parent channel's Discord snowflake
    discord_parent_id bigint,
    -- Owner's Discord snowflake
    discord_owner_id bigint not null,
    name text not null,
    kind text not null,
    archived boolean not null default false,
    locked boolean not null default false,
    pinned boolean not null default false,
    auto_archive_duration integer not null default 1440,  -- minutes
    message_count integer not null default 0,
    member_count integer not null default 0,
    first_message_id bigint,
    -- Forum-specific: applied tag ids (Discord snowflakes)
    applied_tag_ids bigint[],
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

-- Thread membership tracks which users have joined a thread.
-- Mirrors Discord's Thread Member objects.
create table if not exists thread_members (
    thread_channel_id bigint not null references threads(channel_id) on delete cascade,
    discord_user_id bigint not null,
    join_timestamp timestamptz not null default now(),
    flags bigint not null default 0,
    -- Per-user notification settings for this thread
    notification_level bigint not null default 0,
    muted boolean not null default false,
    mute_end_time timestamptz,
    primary key (thread_channel_id, discord_user_id)
);

-- Forum tags for categorizing forum posts.
create table if not exists forum_tags (
    id bigserial primary key,
    discord_guild_id bigint not null,
    discord_id bigint,  -- Discord snowflake tag id (nullable until synced)
    name text not null,
    emoji text,
    moderated boolean not null default false,
    position integer not null default 0
);

-- Junction table for which tags are applied to a thread (forum post).
create table if not exists thread_tags (
    thread_channel_id bigint not null references threads(channel_id) on delete cascade,
    tag_id bigint not null references forum_tags(id) on delete cascade,
    primary key (thread_channel_id, tag_id)
);

-- ============================================================================
-- POLLS
-- ============================================================================

create table if not exists polls (
    id bigserial primary key,
    discord_message_id bigint not null,
    question text not null,
    allow_multiselect boolean not null default false,
    results_finalized boolean,
    created_at timestamptz not null default now()
);

create table if not exists poll_options (
    id bigserial primary key,
    poll_id bigint not null references polls(id) on delete cascade,
    answer_id smallint not null,
    text text not null,
    vote_count bigint not null default 0,
    position smallint not null default 0
);

create table if not exists poll_votes (
    poll_id bigint not null references polls(id) on delete cascade,
    option_id bigint not null references poll_options(id) on delete cascade,
    discord_user_id bigint not null,
    primary key (poll_id, option_id, discord_user_id)
);

-- ============================================================================
-- PINNED MESSAGES
-- ============================================================================

create table if not exists pinned_messages (
    channel_id bigint not null,
    message_id bigint not null references messages(id) on delete cascade,
    pinned_at timestamptz not null default now(),
    primary key (channel_id, message_id)
);

-- ============================================================================
-- RLS POLICIES
-- ============================================================================

alter table threads enable row level security;
alter table thread_members enable row level security;
alter table forum_tags enable row level security;
alter table thread_tags enable row level security;
alter table polls enable row level security;
alter table poll_options enable row level security;
alter table poll_votes enable row level security;
alter table pinned_messages enable row level security;

-- Threads: visible if the user can see the parent channel/guild.
-- We resolve the parent guild via discord_guild_id and use guild_memberships,
-- or for DMs, allow if the user is a DM participant on the parent.
create policy "threads visible in accessible channels"
    on threads for select
    using (
        (discord_guild_id is not null
            and discord_guild_id in (
                select g.guild_id from guilds g
                join guild_memberships gm on gm.guild_id = g.id
                join auth.users u on gm.user_id = u.id
                where u.auth_user_id = auth.uid()
            ))
        or (discord_guild_id is null
            and discord_parent_id in (
                select c.channel_id from channels c
                join dm_participants dm on dm.channel_id = c.id
                join auth.users u on dm.user_id = u.id
                where u.auth_user_id = auth.uid()
            ))
    );

create policy "user can create threads in accessible guilds"
    on threads for insert
    with check (
        discord_guild_id is not null
        and discord_guild_id in (
            select g.guild_id from guilds g
            join guild_memberships gm on gm.guild_id = g.id
            join auth.users u on gm.user_id = u.id
            where u.auth_user_id = auth.uid()
        )
    );

create policy "user can update own threads"
    on threads for update
    using (
        discord_owner_id = (select discord_id from auth.users where auth_user_id = auth.uid())
        or discord_guild_id in (
            -- Allow guild admins/managers to edit. For now, just thread owners.
            select g.guild_id from guilds g
            join guild_memberships gm on gm.guild_id = g.id
            join auth.users u on gm.user_id = u.id
            where u.auth_user_id = auth.uid()
            and gm.role_flags & 8 != 0  -- ADMINISTRATOR
        )
    );

-- Thread members: user can see memberships for threads they can access,
-- and can manage their own membership records.
create policy "thread members visible to thread participants"
    on thread_members for select
    using (
        exists (
            select 1 from threads t
            where t.channel_id = thread_members.thread_channel_id
            and (
                t.discord_guild_id in (
                    select g.guild_id from guilds g
                    join guild_memberships gm on gm.guild_id = g.id
                    join auth.users u on gm.user_id = u.id
                    where u.auth_user_id = auth.uid()
                )
                or (
                    t.discord_guild_id is null
                    and t.discord_parent_id in (
                        select c.channel_id from channels c
                        join dm_participants dm on dm.channel_id = c.id
                        join auth.users u on dm.user_id = u.id
                        where u.auth_user_id = auth.uid()
                    )
                )
            )
        )
    );

create policy "user can manage own thread membership"
    on thread_members for all
    using (discord_user_id = (select discord_id from auth.users where auth_user_id = auth.uid()))
    with check (discord_user_id = (select discord_id from auth.users where auth_user_id = auth.uid()));

-- Forum tags: visible within guilds the user belongs to
create policy "forum tags visible in accessible guilds"
    on forum_tags for select
    using (
        discord_guild_id in (
            select g.guild_id from guilds g
            join guild_memberships gm on gm.guild_id = g.id
            join auth.users u on gm.user_id = u.id
            where u.auth_user_id = auth.uid()
        )
    );

create policy "user can manage forum tags in accessible guilds"
    on forum_tags for all
    using (
        discord_guild_id in (
            select g.guild_id from guilds g
            join guild_memberships gm on gm.guild_id = g.id
            join auth.users u on gm.user_id = u.id
            where u.auth_user_id = auth.uid()
        )
    )
    with check (
        discord_guild_id in (
            select g.guild_id from guilds g
            join guild_memberships gm on gm.guild_id = g.id
            join auth.users u on gm.user_id = u.id
            where u.auth_user_id = auth.uid()
        )
    );

-- Thread tags: inherit visibility from threads
create policy "thread tags visible with thread"
    on thread_tags for select
    using (
        thread_channel_id in (select channel_id from threads where
            (discord_guild_id is not null
                and discord_guild_id in (
                    select g.guild_id from guilds g
                    join guild_memberships gm on gm.guild_id = g.id
                    join auth.users u on gm.user_id = u.id
                    where u.auth_user_id = auth.uid()
                ))
            or (discord_guild_id is null
                and discord_parent_id in (
                    select c.channel_id from channels c
                    join dm_participants dm on dm.channel_id = c.id
                    join auth.users u on dm.user_id = u.id
                    where u.auth_user_id = auth.uid()
                ))
        )
    );

create policy "user can manage thread tags as thread owner"
    on thread_tags for insert
    with check (
        thread_channel_id in (
            select channel_id from threads
            where discord_owner_id = (select discord_id from auth.users where auth_user_id = auth.uid())
        )
    );

-- Polls: visible in channels the user can access
create policy "polls visible in accessible channels"
    on polls for select
    using (
        discord_message_id in (
            select id from messages m
            where m.channel_id in (
                select c.channel_id from channels c
                where (c.guild_id is not null
                    and c.guild_id in (
                        select g.id from guilds g
                        join guild_memberships gm on gm.guild_id = g.id
                        join auth.users u on gm.user_id = u.id
                        where u.auth_user_id = auth.uid()
                    ))
                    or (c.guild_id is null
                        and c.channel_id in (
                            select dm.channel_id from dm_participants dm
                            join auth.users u on dm.user_id = u.id
                            where u.auth_user_id = auth.uid()
                        ))
            )
        )
    );

create policy "user can create polls on own messages"
    on polls for insert
    with check (
        discord_message_id in (
            select m.id from messages m
            where m.author_id = (select discord_id from auth.users where auth_user_id = auth.uid())
            and m.channel_id in (
                select c.channel_id from channels c
                where (c.guild_id is not null
                    and c.guild_id in (
                        select g.id from guilds g
                        join guild_memberships gm on gm.guild_id = g.id
                        join auth.users u on gm.user_id = u.id
                        where u.auth_user_id = auth.uid()
                    ))
            )
        )
    );

-- Poll votes: user can only manage their own votes
create policy "user can manage own poll votes"
    on poll_votes for all
    using (discord_user_id = (select discord_id from auth.users where auth_user_id = auth.uid()))
    with check (discord_user_id = (select discord_id from auth.users where auth_user_id = auth.uid()));

-- Pinned messages: visible in accessible channels
create policy "pinned messages visible in accessible channels"
    on pinned_messages for select
    using (
        channel_id in (
            select c.channel_id from channels c
            where (c.guild_id is not null
                and c.guild_id in (
                    select g.id from guilds g
                    join guild_memberships gm on gm.guild_id = g.id
                    join auth.users u on gm.user_id = u.id
                    where u.auth_user_id = auth.uid()
                ))
                or (c.guild_id is null
                    and c.channel_id in (
                        select dm.channel_id from dm_participants dm
                        join auth.users u on dm.user_id = u.id
                        where u.auth_user_id = auth.uid()
                    ))
        )
    );

create policy "user can manage pins in guild channels"
    on pinned_messages for insert
    with check (
        channel_id in (
            select c.channel_id from channels c
            where c.guild_id is not null
            and c.guild_id in (
                select g.id from guilds g
                join guild_memberships gm on gm.guild_id = g.id
                join auth.users u on gm.user_id = u.id
                where u.auth_user_id = auth.uid()
            )
        )
    );

create policy "user can delete pins in guild channels"
    on pinned_messages for delete
    using (
        channel_id in (
            select c.channel_id from channels c
            where c.guild_id is not null
            and c.guild_id in (
                select g.id from guilds g
                join guild_memberships gm on gm.guild_id = g.id
                join auth.users u on gm.user_id = u.id
                where u.auth_user_id = auth.uid()
            )
        )
    );
