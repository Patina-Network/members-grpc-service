CREATE TYPE "MemberRole" AS ENUM ('board', 'mentee', 'mentor');

ALTER TABLE "Member"
    DROP CONSTRAINT "Member_role_check",
    ALTER COLUMN role TYPE "MemberRole" USING role::"MemberRole";
