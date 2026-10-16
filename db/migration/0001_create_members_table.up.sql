CREATE TABLE "Member" (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    "firstName" TEXT NOT NULL,
    "lastName" TEXT NOT NULL,
    "joinedAt" TIMESTAMP WITH TIME ZONE NOT NULL,
    "discordId" TEXT UNIQUE,
    "discordUsername" TEXT,
    "updatedAt" TIMESTAMP WITH TIME ZONE NOT NULL,
    "archivedAt" TIMESTAMP WITH TIME ZONE
);
