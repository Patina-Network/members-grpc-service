UPDATE "Member" SET
    "linkedinUsername" = NULL,
    "instagramUsername" = NULL,
    career = NULL,
    industry = NULL,
    profile = NULL
WHERE "discordId" IN (
    '100000000000000001',
    '100000000000000002',
    '100000000000000003',
    '100000000000000004',
    '100000000000000005'
);
