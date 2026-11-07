CREATE TYPE "CommunicationMethod" AS ENUM (
    'Email', 'PhoneNumber', 'Discord', 'Instagram', 'LinkedIn'
);

CREATE TABLE "UserPreferredCommunication" (
    "memberId" INTEGER NOT NULL REFERENCES "Member"(id) ON DELETE CASCADE,
    method "CommunicationMethod" NOT NULL,
    PRIMARY KEY ("memberId", method)
);
