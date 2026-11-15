CREATE TABLE "UserEmails" (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    "memberId" INTEGER NOT NULL REFERENCES "Member" (id) ON DELETE CASCADE,
    email TEXT NOT NULL UNIQUE,
    "isPrimary" BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE UNIQUE INDEX "UserEmails_memberId_isPrimary_idx"
    ON "UserEmails" ("memberId")
    WHERE "isPrimary";

CREATE INDEX "UserEmails_memberId_idx" ON "UserEmails" ("memberId");

INSERT INTO "UserEmails" ("memberId", email, "isPrimary")
SELECT id, email, TRUE FROM "Member";