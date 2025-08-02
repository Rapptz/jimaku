CREATE TABLE IF NOT EXISTS report (
    -- This ID is a timestamp in ms
    id INTEGER PRIMARY KEY,
    account_id INTEGER REFERENCES account(id) ON DELETE CASCADE,
    entry_id INTEGER REFERENCES directory_entry(id) ON DELETE SET NULL,
    status INTEGER NOT NULL DEFAULT (0),
    reason TEXT NOT NULL,
    response TEXT,
    -- This is a JSON payload with the report data
    payload TEXT
);

CREATE INDEX IF NOT EXISTS report_account_id_idx ON report(account_id);
CREATE INDEX IF NOT EXISTS report_entry_id_idx ON report(entry_id);
CREATE INDEX IF NOT EXISTS report_status_idx ON report(status);

PRAGMA user_version = 4;
