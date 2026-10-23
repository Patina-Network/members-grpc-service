ALTER TABLE "Member"
    ADD COLUMN "linkedinUsername" TEXT CONSTRAINT "Member_linkedinUsername_key" UNIQUE,
    ADD COLUMN "instagramUsername" TEXT CONSTRAINT "Member_instagramUsername_key" UNIQUE,
    ADD COLUMN career TEXT,
    ADD COLUMN industry TEXT,
    ADD COLUMN profile TEXT;
