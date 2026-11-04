CREATE TYPE communication_method AS ENUM (
    'Email', 'PhoneNumber', 'Discord', 'Instagram', 'LinkedIn'
);

CREATE TABLE "UserPreferredCommunication" (
    "memberId" INTEGER NOT NULL REFERENCES "Member"(id) ON DELETE CASCADE,
    method communication_method NOT NULL,
    PRIMARY KEY ("memberId", method)
);
