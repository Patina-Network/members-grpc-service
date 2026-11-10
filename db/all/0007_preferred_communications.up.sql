CREATE TYPE communication_method AS ENUM (
    'Email', 'PhoneNumber', 'Discord', 'Instagram', 'LinkedIn'
);

CREATE TABLE user_preferred_communication (
    member_id INTEGER NOT NULL REFERENCES "Member"(id) ON DELETE CASCADE,
    method communication_method NOT NULL,
    PRIMARY KEY (member_id, method)
);
