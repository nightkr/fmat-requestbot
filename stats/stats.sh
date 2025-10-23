#!/usr/bin/env bash

set -euo pipefail

echo "Requests completed during $WARNUMBER"
sudo -u postgres -D / psql --tuples-only --no-align fmat_requestbot -v "warstart='$WARSTART'" -v "guild=$GUILD" < stats-requests.sql
echo "Tasks completed during $WARNUMBER"
sudo -u postgres -D / psql --tuples-only --no-align fmat_requestbot -v "warstart='$WARSTART'" -v "guild=$GUILD" < stats-tasks.sql
echo "Requests created during $WARNUMBER"
sudo -u postgres -D / psql --tuples-only --no-align fmat_requestbot -v "warstart='$WARSTART'" -v "guild=$GUILD" < stats-created.sql
