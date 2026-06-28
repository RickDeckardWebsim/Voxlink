-- VoxLink Replies + Mentions — SQL Migration
-- Run once in the Supabase dashboard SQL editor.
-- Adds reply reference columns to the messages table.
-- The messages.id column already accepts explicit client-generated UUIDs
-- (DEFAULT gen_random_uuid() only fires when id is omitted) — no change needed there.

ALTER TABLE messages ADD COLUMN IF NOT EXISTS reply_to_id      UUID REFERENCES messages(id) ON DELETE SET NULL;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS reply_to_author  TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS reply_to_content TEXT;

-- Verify:
-- SELECT column_name, data_type FROM information_schema.columns
--   WHERE table_name = 'messages' AND column_name LIKE 'reply_to_%';
