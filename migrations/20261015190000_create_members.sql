CREATE TABLE IF NOT EXISTS members (
    member_id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    joined_at TIMESTAMP WITH TIME ZONE NOT NULL,
    discord_id TEXT,
    discord_username TEXT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL,
    archived_at TIMESTAMP WITH TIME ZONE,
    CONSTRAINT members_email_key UNIQUE (email),
    CONSTRAINT members_discord_id_key UNIQUE (discord_id)
);
