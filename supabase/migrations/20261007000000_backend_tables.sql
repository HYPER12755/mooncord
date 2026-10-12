create table user_settings (
    discord_user_id bigint primary key,
    selected_guild_id bigint,
    selected_channel_id bigint,
    updated_at timestamptz not null default now()
);

alter table user_settings enable row level security;

create policy "user can read own settings"
    on user_settings for select
    using (discord_user_id = (select discord_id from public.users where id = auth.uid()));

create policy "user can upsert own settings"
    on user_settings for all
    using (discord_user_id = (select discord_id from public.users where id = auth.uid()))
    with check (discord_user_id = (select discord_id from public.users where id = auth.uid()));

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
    using (discord_user_id = (select discord_id from public.users where id = auth.uid()))
    with check (discord_user_id = (select discord_id from public.users where id = auth.uid()));

create policy "user can manage own channel mutes"
    on channel_mutes for all
    using (discord_user_id = (select discord_id from public.users where id = auth.uid()))
    with check (discord_user_id = (select discord_id from public.users where id = auth.uid()));
