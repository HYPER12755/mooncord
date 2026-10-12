-- Phase 5: Media storage
-- The attachments table was created in Phase 0 with columns (id, message_id,
-- url, filename, size, content_type, created_at) and basic RLS that delegates
-- visibility to the messages table's own RLS. Phase 5 tightens the RLS policy
-- and adds a discord_message_id column for Realtime payloads.

-- Add discord_message_id for Realtime (snowflake) payloads.
alter table attachments add column if not exists discord_message_id bigint;

-- Phase 0's policy delegates attachment visibility to messages' RLS, which is
-- correct but indirect. Keep it — no need to add a redundant policy.
-- The insert policy from Phase 0 allows any authenticated user to insert;
-- tighten it so only the message author can attach.
drop policy if exists "attachments insertable by message author" on attachments;
create policy "attachments insertable by message author"
    on attachments for insert
    with check (
        (select author_id from messages where id = attachments.message_id) =
        (select id from public.users where auth_user_id = auth.uid())
    );
