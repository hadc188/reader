-- Remove the AI feature (deleted in v1.2.0): drop the enable_ai_model column
-- and the ai_book_memories table. Kept as a separate migration so existing
-- databases (which already applied 0003) migrate forward cleanly instead of
-- tripping sqlx's checksum validation on the edited 0003.

ALTER TABLE users DROP COLUMN enable_ai_model;

DROP TABLE IF EXISTS ai_book_memories;
