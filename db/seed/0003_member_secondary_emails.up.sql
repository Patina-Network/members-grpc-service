INSERT INTO "UserEmails" ("memberId", email, "isPrimary")
SELECT m.id, v.email, v.is_primary
FROM (VALUES
    ('100000000000000001', 'member1@example.com', TRUE),
    ('100000000000000001', 'member1-secondary@example.com', FALSE),
    ('100000000000000001', 'member1-alt@example.com', FALSE),
    ('100000000000000002', 'member2@example.com', TRUE),
    ('100000000000000003', 'member3@example.com', TRUE),
    ('100000000000000003', 'member3-secondary@example.com', FALSE),
    ('100000000000000004', 'member4@example.com', TRUE),
    ('100000000000000005', 'member5@example.com', TRUE)
) AS v (discord_id, email, is_primary)
JOIN "Member" AS m ON m."discordId" = v.discord_id
ON CONFLICT (email) DO NOTHING;
