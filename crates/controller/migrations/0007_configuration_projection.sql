ALTER TABLE controller_settings ADD COLUMN server_host TEXT NOT NULL DEFAULT '127.0.0.1';
ALTER TABLE controller_settings ADD COLUMN server_port INTEGER NOT NULL DEFAULT 3001 CHECK (server_port BETWEEN 1 AND 65535);
ALTER TABLE controller_settings ADD COLUMN secure_cookie INTEGER NOT NULL DEFAULT 0 CHECK (secure_cookie IN (0, 1));
ALTER TABLE controller_settings ADD COLUMN session_absolute_seconds INTEGER NOT NULL DEFAULT 86400 CHECK (session_absolute_seconds > 0);
ALTER TABLE controller_settings ADD COLUMN session_idle_seconds INTEGER NOT NULL DEFAULT 3600 CHECK (session_idle_seconds > 0 AND session_idle_seconds <= session_absolute_seconds);
ALTER TABLE controller_settings ADD COLUMN config_document TEXT NOT NULL DEFAULT '';
ALTER TABLE controller_settings ADD COLUMN pending_config_document TEXT;
ALTER TABLE controller_settings ADD COLUMN configuration_initialized INTEGER NOT NULL DEFAULT 0 CHECK (configuration_initialized IN (0, 1));
