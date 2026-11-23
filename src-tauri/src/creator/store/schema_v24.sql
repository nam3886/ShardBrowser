-- Ghidorah Creator store, schema user_version 24: `sqlite3 x .schema` of a fresh database
-- migrated by the Python app (ghidorah-creator commit fae8189, store/schema.py), verbatim minus
-- sqlite_stat1/sqlite_stat4 (made by ANALYZE). The revs rows at the end are the v24 migration's
-- seed data (.schema prints no rows); every trigger bumps one of them.
CREATE TABLE mailboxes (
        id INTEGER PRIMARY KEY,
        provider TEXT NOT NULL DEFAULT 'ele',
        email TEXT NOT NULL,
        domain TEXT NOT NULL,
        token TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'active',
        created_at TEXT NOT NULL,
        expires_at TEXT
    , google_used INTEGER NOT NULL DEFAULT 0, client_id TEXT);
CREATE INDEX idx_mailboxes_email ON mailboxes(email);
CREATE TABLE proxies (
        id INTEGER PRIMARY KEY,
        host TEXT NOT NULL,
        port INTEGER NOT NULL,
        username TEXT,
        password TEXT,
        active INTEGER NOT NULL DEFAULT 1,
        status TEXT NOT NULL DEFAULT 'unknown',
        reason TEXT,
        latency_ms INTEGER,
        exit_ip TEXT,
        country TEXT,
        fail_streak INTEGER NOT NULL DEFAULT 0,
        checked_at TEXT,
        signups_ok INTEGER NOT NULL DEFAULT 0,
        signups_total INTEGER NOT NULL DEFAULT 0,
        last_used_at TEXT,
        created_at TEXT NOT NULL, org TEXT, refused_at TEXT, kind TEXT NOT NULL DEFAULT 'static',
        UNIQUE (host, port, username)
    );
CREATE TABLE mail_messages (
        id INTEGER PRIMARY KEY,
        mailbox_id INTEGER NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
        ele_message_id TEXT,
        sender TEXT,
        subject TEXT,
        body TEXT,
        otp_extracted TEXT,
        received_at TEXT
    , html TEXT, stored_at TEXT);
CREATE UNIQUE INDEX ux_mail_msg_id ON mail_messages(mailbox_id, ele_message_id)
        WHERE ele_message_id IS NOT NULL;
CREATE TABLE blocked_domains (
        id INTEGER PRIMARY KEY,
        domain TEXT NOT NULL UNIQUE,
        reason TEXT,
        added_at TEXT NOT NULL
    );
CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    platform TEXT NOT NULL,
    choice TEXT NOT NULL DEFAULT 'auto',
    count INTEGER NOT NULL DEFAULT 0,
    proxy_mode TEXT NOT NULL DEFAULT 'rotate',
    created_at TEXT NOT NULL
, proxy_kind TEXT NOT NULL DEFAULT '');
CREATE TABLE browsers (
    id INTEGER PRIMARY KEY,
    mailbox_id INTEGER NOT NULL UNIQUE REFERENCES mailboxes(id) ON DELETE CASCADE,
    name TEXT,
    path TEXT,
    proxy_id INTEGER REFERENCES proxies(id) ON DELETE SET NULL,
    prev_proxy_id INTEGER,
    moved_at TEXT,
    proxy_mode TEXT NOT NULL DEFAULT 'rotate',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    mailbox_id INTEGER NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    route TEXT NOT NULL,
    parent_id INTEGER REFERENCES accounts(id) ON DELETE NO ACTION,
    dedupe_key TEXT NOT NULL DEFAULT '',
    login TEXT,
    secrets TEXT NOT NULL DEFAULT '{}',
    extra TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'queued',
    step TEXT,
    step_note TEXT,
    error TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    retry_at TEXT,
    screenshot TEXT,
    verify_status TEXT,
    verified_at TEXT,
    cookies TEXT,
    cookies_at TEXT,
    frozen INTEGER NOT NULL DEFAULT 0,
    relay_id TEXT,
    job_id INTEGER REFERENCES jobs(id) ON DELETE SET NULL,
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL, error_code TEXT,
    UNIQUE (mailbox_id, platform, dedupe_key)
);
CREATE INDEX idx_accounts_platform_status ON accounts(platform, status);
CREATE INDEX idx_accounts_parent ON accounts(parent_id);
CREATE INDEX idx_accounts_job ON accounts(job_id);
CREATE TABLE platform_settings (
    platform TEXT PRIMARY KEY,
    account_rule TEXT,
    starred INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE account_checks (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    event TEXT NOT NULL,
    source TEXT NOT NULL,
    proxy_id INTEGER,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_account_checks_account ON account_checks(account_id)
;
CREATE TABLE IF NOT EXISTS "events" (
    id INTEGER PRIMARY KEY,
    browser_id INTEGER REFERENCES browsers(id) ON DELETE SET NULL,
    mailbox_id INTEGER REFERENCES mailboxes(id) ON DELETE SET NULL,
    level TEXT NOT NULL,
    step TEXT,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS "proxy_moves" (
    id INTEGER PRIMARY KEY,
    browser_id INTEGER NOT NULL REFERENCES browsers(id) ON DELETE CASCADE,
    from_proxy_id INTEGER REFERENCES proxies(id) ON DELETE SET NULL,
    to_proxy_id INTEGER REFERENCES proxies(id) ON DELETE SET NULL,
    pending INTEGER NOT NULL DEFAULT 1,
    undone INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_mailboxes_provider_status ON mailboxes(provider, status);
CREATE INDEX idx_mailboxes_status_domain ON mailboxes(status, domain);
CREATE INDEX idx_mail_mailbox ON mail_messages(mailbox_id, received_at);
CREATE INDEX idx_accounts_status ON accounts(status, frozen);
CREATE INDEX idx_accounts_mb_platform ON accounts(mailbox_id, platform, status);
CREATE INDEX idx_accounts_finished ON accounts(finished_at);
CREATE INDEX idx_browsers_proxy ON browsers(proxy_id);
CREATE INDEX idx_browsers_path ON browsers(path);
CREATE INDEX idx_proxies_state ON proxies(active, status, kind);
CREATE INDEX idx_events_mailbox ON events(mailbox_id);
CREATE INDEX idx_events_browser ON events(browser_id);
CREATE INDEX idx_events_created ON events(created_at);
CREATE INDEX idx_moves_browser ON proxy_moves(browser_id, pending);
CREATE INDEX idx_moves_to ON proxy_moves(to_proxy_id);
CREATE INDEX idx_moves_created ON proxy_moves(created_at)
;
CREATE TABLE revs (name TEXT PRIMARY KEY, rev INTEGER NOT NULL DEFAULT 0);
CREATE TRIGGER rev_mailboxes_insert AFTER INSERT ON mailboxes BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('mailboxes', 'mailboxes_plan'); END;
CREATE TRIGGER rev_mailboxes_delete AFTER DELETE ON mailboxes BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('mailboxes', 'mailboxes_plan'); END;
CREATE TRIGGER rev_mailboxes_update AFTER UPDATE ON mailboxes BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'mailboxes'; END;
CREATE TRIGGER rev_mailboxes_plan AFTER UPDATE OF status, provider, domain, google_used, token, client_id ON mailboxes WHEN old.status IS NOT new.status OR old.provider IS NOT new.provider OR old.domain IS NOT new.domain OR old.google_used IS NOT new.google_used OR (old.token = '') IS NOT (new.token = '') OR (COALESCE(old.client_id, '') = '') IS NOT (COALESCE(new.client_id, '') = '') BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'mailboxes_plan'; END;
CREATE TRIGGER rev_accounts_insert AFTER INSERT ON accounts BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('accounts', 'accounts_plan'); END;
CREATE TRIGGER rev_accounts_delete AFTER DELETE ON accounts BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('accounts', 'accounts_plan'); END;
CREATE TRIGGER rev_accounts_update AFTER UPDATE ON accounts BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'accounts'; END;
CREATE TRIGGER rev_accounts_plan AFTER UPDATE OF status, frozen, error_code, dedupe_key, platform, mailbox_id, parent_id, verify_status ON accounts WHEN old.status IS NOT new.status OR old.frozen IS NOT new.frozen OR old.error_code IS NOT new.error_code OR old.dedupe_key IS NOT new.dedupe_key OR old.platform IS NOT new.platform OR old.mailbox_id IS NOT new.mailbox_id OR old.parent_id IS NOT new.parent_id OR old.verify_status IS NOT new.verify_status BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'accounts_plan'; END;
CREATE TRIGGER rev_browsers_insert AFTER INSERT ON browsers BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('browsers'); END;
CREATE TRIGGER rev_browsers_delete AFTER DELETE ON browsers BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('browsers'); END;
CREATE TRIGGER rev_browsers_update AFTER UPDATE ON browsers BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'browsers'; END;
CREATE TRIGGER rev_proxies_insert AFTER INSERT ON proxies BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('proxies'); END;
CREATE TRIGGER rev_proxies_delete AFTER DELETE ON proxies BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('proxies'); END;
CREATE TRIGGER rev_proxies_update AFTER UPDATE ON proxies BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'proxies'; END;
CREATE TRIGGER rev_mail_messages_insert AFTER INSERT ON mail_messages BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('mail_messages'); END;
CREATE TRIGGER rev_mail_messages_delete AFTER DELETE ON mail_messages BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('mail_messages'); END;
CREATE TRIGGER rev_mail_messages_update AFTER UPDATE ON mail_messages BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'mail_messages'; END;
CREATE TRIGGER rev_jobs_insert AFTER INSERT ON jobs BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('jobs'); END;
CREATE TRIGGER rev_jobs_delete AFTER DELETE ON jobs BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('jobs'); END;
CREATE TRIGGER rev_jobs_update AFTER UPDATE ON jobs BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'jobs'; END;
CREATE TRIGGER rev_proxy_moves_insert AFTER INSERT ON proxy_moves BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('proxy_moves'); END;
CREATE TRIGGER rev_proxy_moves_delete AFTER DELETE ON proxy_moves BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('proxy_moves'); END;
CREATE TRIGGER rev_proxy_moves_update AFTER UPDATE ON proxy_moves BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'proxy_moves'; END;
CREATE TRIGGER rev_blocked_domains_insert AFTER INSERT ON blocked_domains BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('blocked_domains'); END;
CREATE TRIGGER rev_blocked_domains_delete AFTER DELETE ON blocked_domains BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('blocked_domains'); END;
CREATE TRIGGER rev_blocked_domains_update AFTER UPDATE ON blocked_domains BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'blocked_domains'; END;
CREATE TRIGGER rev_settings_insert AFTER INSERT ON settings BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('settings'); END;
CREATE TRIGGER rev_settings_delete AFTER DELETE ON settings BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('settings'); END;
CREATE TRIGGER rev_settings_update AFTER UPDATE ON settings BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'settings'; END;
CREATE TRIGGER rev_platform_settings_insert AFTER INSERT ON platform_settings BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('platform_settings'); END;
CREATE TRIGGER rev_platform_settings_delete AFTER DELETE ON platform_settings BEGIN UPDATE revs SET rev = rev + 1 WHERE name IN ('platform_settings'); END;
CREATE TRIGGER rev_platform_settings_update AFTER UPDATE ON platform_settings BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'platform_settings'; END;
CREATE TRIGGER rev_secrets_acc_ins AFTER INSERT ON accounts WHEN new.secrets <> '{}' BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_acc_del AFTER DELETE ON accounts WHEN old.secrets <> '{}' BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_acc_upd AFTER UPDATE OF secrets ON accounts WHEN old.secrets IS NOT new.secrets BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_px_ins AFTER INSERT ON proxies WHEN new.password IS NOT NULL BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_px_del AFTER DELETE ON proxies WHEN old.password IS NOT NULL BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_px_upd AFTER UPDATE OF password ON proxies WHEN old.password IS NOT new.password BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_set_ins AFTER INSERT ON settings WHEN new.key IN ('ele_api_key', 'relay_api_key', 'yescaptcha_key', 'shard_api_token') BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_set_del AFTER DELETE ON settings WHEN old.key IN ('ele_api_key', 'relay_api_key', 'yescaptcha_key', 'shard_api_token') BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
CREATE TRIGGER rev_secrets_set_upd AFTER UPDATE ON settings WHEN new.key IN ('ele_api_key', 'relay_api_key', 'yescaptcha_key', 'shard_api_token') OR old.key IN ('ele_api_key', 'relay_api_key', 'yescaptcha_key', 'shard_api_token') BEGIN UPDATE revs SET rev = rev + 1 WHERE name = 'secrets'; END;
INSERT INTO revs(name) VALUES
    ('mailboxes'),
    ('accounts'),
    ('browsers'),
    ('proxies'),
    ('mail_messages'),
    ('jobs'),
    ('proxy_moves'),
    ('blocked_domains'),
    ('settings'),
    ('platform_settings'),
    ('accounts_plan'),
    ('mailboxes_plan'),
    ('secrets');
