UPDATE "Member" SET
    "linkedinUsername" = NULL,
    "instagramUsername" = NULL,
    career = NULL,
    industry = NULL,
    profile = NULL
WHERE email IN (
    'member1@example.com',
    'member2@example.com',
    'member3@example.com',
    'member4@example.com',
    'member5@example.com'
);
