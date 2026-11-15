ALTER TABLE "Member" ADD COLUMN email TEXT;

UPDATE "Member" m
SET email = ue.email
FROM "UserEmails" ue
WHERE ue."memberId" = m.id
AND ue."isPrimary";