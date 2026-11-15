INSERT INTO "UserEmails" ("memberId", email, "isPrimary")
SELECT m.id, v.email, v.is_primary
FROM (VALUES
    ('member1@example.com', 'member1@example.com', TRUE),
    ('member1@example.com', 'member1-secondary@example.com', FALSE),
    ('member1@example.com', 'member1-alt@example.com', FALSE),
    ('member2@example.com', 'member2@example.com', TRUE),
    ('member3@example.com', 'member3@example.com', TRUE),
    ('member3@example.com', 'member3-secondary@example.com', FALSE),
    ('member4@example.com', 'member4@example.com', TRUE),
    ('member5@example.com', 'member5@example.com', TRUE)
) AS v (member_email, email, is_primary)
JOIN "Member" AS m ON m.email = v.member_email
ON CONFLICT (email) DO NOTHING;
