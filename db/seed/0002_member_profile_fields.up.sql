UPDATE "Member" AS m SET
    "linkedinUsername" = v.linkedin_username,
    "instagramUsername" = v.instagram_username,
    career = v.career,
    industry = v.industry,
    profile = v.profile
FROM (VALUES
    ('100000000000000001', 'member-one', 'member.one', 'Software Engineer', 'Technology', 'Mentor for new members.'),
    ('100000000000000002', 'member-two', NULL, 'Software Engineer', 'Finance', NULL),
    ('100000000000000003', NULL, 'member.three', 'Product Manager', 'Technology', NULL),
    ('100000000000000004', 'member-four', NULL, 'Data Scientist', 'Healthcare', NULL),
    ('100000000000000005', NULL, NULL, 'Product Manager', 'Technology', NULL)
) AS v (discord_id, linkedin_username, instagram_username, career, industry, profile)
WHERE m."discordId" = v.discord_id;
