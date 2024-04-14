PRAGMA foreign_keys = ON;
PRAGMA journal_mode = wal;

-- Storage related information for directory entries
-- This has to be a little bit complicated to power the fact
-- that multiple searches are allowed

-- Do note that SQLite does *not* allow atomic ALTER TABLE
-- So this schema is pretty much final unless you leave it
-- to atomic operations like CREATE TABLE and CREATE INDEX

CREATE TABLE IF NOT EXISTS directory_entry (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  last_updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  creator_id INTEGER REFERENCES account(id) ON DELETE SET NULL,
  flags INTEGER NOT NULL DEFAULT 1,
  anilist_id INTEGER UNIQUE,
  tmdb_id TEXT UNIQUE,
  notes TEXT,
  english_name TEXT,
  japanese_name TEXT,
  name TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS directory_entry_path_idx ON directory_entry(path);
CREATE INDEX IF NOT EXISTS directory_entry_flags_idx ON directory_entry(flags);
CREATE INDEX IF NOT EXISTS directory_entry_anilist_id_idx ON directory_entry(anilist_id);
CREATE INDEX IF NOT EXISTS directory_entry_creator_id_idx ON directory_entry(creator_id);
CREATE INDEX IF NOT EXISTS directory_entry_tmdb_id_idx ON directory_entry(tmdb_id);

-- This is for the authentication aspect
-- Note that usernames are all lowercase
-- Email is *not* stored anywhere
CREATE TABLE IF NOT EXISTS account (
  id INTEGER PRIMARY KEY,
  name TEXT UNIQUE NOT NULL,
  password TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  flags INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS account_name_idx ON account(name);

-- This is a long form key: value storage that can be used for any type of
-- generic data that doesn't belong in a normalized table.
-- Due to the dynamic typing nature of SQLite that we're abusing, the value
-- can technically have any type.
CREATE TABLE IF NOT EXISTS storage(
  name TEXT PRIMARY KEY,
  value TEXT
) WITHOUT ROWID;


CREATE TABLE IF NOT EXISTS session (
  id TEXT PRIMARY KEY,
  account_id INTEGER REFERENCES account(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  description TEXT,
  api_key INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS session_account_id_idx ON session(account_id);
CREATE INDEX IF NOT EXISTS session_api_key_idx ON session(api_key);
