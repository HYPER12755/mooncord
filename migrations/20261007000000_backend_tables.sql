create table user_settings (
    discord_user_id bigint primary key,
    selected_guild_id bigint,
    selected_channel_id bigint,
    updated_at timestamptz not null default now()
);

drop table if exists typing_indicators;
create table typing_indicators (
    discord_channel_id bigint not null,
    discord_user_id bigint not null,
    expires_at timestamptz not null,
    primary key (discord_channel_id, discord_user_id)
);

alter table user_settings enable row level security;
alter table typing_indicators enable row level security;

create policy "user can read own settings"
    on user_settings for select
    using (discord_user_id = (select discord_id from auth.users where id = auth.uid()));

create policy "user can upsert own settings"
    on user_settings for all
    using (discord_user_id = (select discord_id from auth.users where id = auth.uid()))
    with check (discord_user_id = (select discord_id from auth.users where id = auth.uid()));

create policy "typing visible in accessible channels"
    on typing_indicators for select
    using (
        discord_channel_id in (
            select discord_channel_id from channels
            where (guild_id is not null and discord_guild_id in (
                select g.guild_id from guilds g
                join guild_memberships gm on gm.discord_guild_id = g.guild_id
                where gm.discord_user_id = (select discord_id from auth.users where id = auth.uid())
            ))
            or (guild_id is null and discord_channel_id in (
                select discord_channel_id from channels c
                join dm_participants dm on dm.channel_id = c.id
                where dm.discord_user_id = (select discord_id from auth.users where id = auth.uid())
            ))
        )
    );

create table guild_mutes (
    discord_user_id bigint not null,
    discord_guild_id bigint not null,
    expires_at timestamptz,
    created_at timestamptz not null default now(),
    primary key (discord_user_id, discord_guild_id)
);

create table channel_mutes (
    discord_user_id bigint not null,
    discord_channel_id bigint not null,
    discord_guild_id bigint,
    expires_at timestamptz,
    created_at timestamptz not null default now(),
    primary key (discord_user_id, discord_channel_id)
);

alter table guild_mutes enable row level security;
alter table channel_mutes enable row level security;

create policy "user can manage own guild mutes"
    on guild_mutes for all
    using (discord_user_id = (select discord_id from auth.users where id = auth.uid()))
    with check (discord_user_id = (select discord_id from auth.users where id = auth.uid()));

create policy "user can manage own channel mutes"
    on channel_mutes for all
    using (discord_user_id = (select discord_id from auth.users where id = auth.uid()))
    with check (discord_user_id = (select discord_id from auth.users where id = auth.uid()));
