-- Per-file thumbnail. Client-generated 200×200 (or so) data URI for image
-- uploads; null for everything else. Phase-2 swaps this for a sandboxed
-- server-side worker (pipeline §5.4) and dedicated thumbnail storage.

ALTER TABLE files ADD COLUMN thumbnail TEXT;
