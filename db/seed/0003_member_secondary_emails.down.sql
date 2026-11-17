DELETE FROM "UserEmails"
WHERE "memberId" IN (
    SELECT id FROM "Member"
    WHERE "discordId" IN (
        '100000000000000001',
        '100000000000000002',
        '100000000000000003',
        '100000000000000004',
        '100000000000000005'
    )
);
