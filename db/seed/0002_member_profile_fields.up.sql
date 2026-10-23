UPDATE "Member" AS m SET
    "linkedinUsername" = v.linkedin_username,
    "instagramUsername" = v.instagram_username,
    career = v.career,
    industry = v.industry,
    profile = v.profile
FROM (VALUES
    ('member1@example.com', 'member-one', 'member.one', 'Software Engineer', 'Technology', 'Mentor for new members.'),
    ('member2@example.com', 'member-two', NULL, 'Software Engineer', 'Finance', NULL),
    ('member3@example.com', NULL, 'member.three', 'Product Manager', 'Technology', NULL),
    ('member4@example.com', 'member-four', NULL, 'Data Scientist', 'Healthcare', NULL),
    ('member5@example.com', NULL, NULL, 'Product Manager', 'Technology', NULL)
) AS v (email, linkedin_username, instagram_username, career, industry, profile)
WHERE m.email = v.email;
