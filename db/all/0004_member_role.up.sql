ALTER TABLE "Member"
    ADD COLUMN role TEXT
        CONSTRAINT "Member_role_check"
        CHECK (role IN ('board', 'mentee', 'mentor'));
