ALTER TABLE "Member"
    ALTER COLUMN role TYPE TEXT USING role::TEXT,
    ADD CONSTRAINT "Member_role_check" CHECK (role IN ('board', 'mentee', 'mentor'));

DROP TYPE "MemberRole";
