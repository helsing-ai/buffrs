#!/bin/bash
set -e
export PGPASSWORD=$POSTGRES_PASSWORD
echo "Applying migrations..."
for file in "$1"/*up.sql
do
    echo "Applying migration $file"
    psql --username "$POSTGRES_USER" --dbname "buffrs" --port 5432 -h "$POSTGRES_HOST" -f "$file"
done
echo "Migrations applied!"
