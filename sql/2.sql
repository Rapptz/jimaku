CREATE TABLE IF NOT EXISTS notification (
    id INTEGER PRIMARY KEY,
    ts INTEGER NOT NULL,
    entry_id INTEGER REFERENCES directory_entry(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    payload TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS notification_ts_idx ON notification(ts);
CREATE INDEX IF NOT EXISTS notification_user_id_idx ON notification(user_id);
CREATE INDEX IF NOT EXISTS notification_entry_id_idx ON notification(entry_id);

CREATE TABLE IF NOT EXISTS bookmark (
    user_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    entry_id INTEGER NOT NULL REFERENCES directory_entry(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, entry_id)
);

-- user_id searches already uses the primary key index but sole entry_id searches do not
CREATE INDEX IF NOT EXISTS bookmark_entry_id_idx ON bookmark(entry_id);

ALTER TABLE account ADD COLUMN notification_ack INTEGER;

PRAGMA user_version = 3;
