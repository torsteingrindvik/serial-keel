#!/bin/bash
# This script will either setup a fresh serial-keel service instance, or upgrade your existing one.
# This will also be executed during every bootup, so a check for an upgrade will be performed.
set -e

# The name of the branch can be specified as a argument, or it defaults to 'main'
BRANCH=${1-main}
REPO_ROOT=$(git rev-parse --show-toplevel)

# Check if there are new changes. If there are none, and if the serial-keel service
# already exists on the machine, then start the serial-keel server
GIT_NO_BRANCH_SWITCH=$(git checkout $BRANCH 2>&1 | grep -i "Already" || true)
GIT_NO_NEW_REMOTE_CHANGES=$(git pull 2>&1 | grep -i "Already up to date" || true)

if [[ -n $GIT_NO_NEW_REMOTE_CHANGES && -n $GIT_NO_BRANCH_SWITCH ]] && test -f /etc/systemd/system/serial-keel.service; then
    echo "serial-keel already up to date."
    exit 0
fi

# If not, we have a serial-keel update
echo "We have a serial-keel update to perform.."

echo "STEP 1/4: Copying the serial-keel.service file, \
in case we have a fresh installation, or if there were updates"

# This replaces all occurences of [user-name-here] and [branch-here] in the serial-keel.template file and 
# with the current user and branch name and places the service file in /etc/systemd/system
CURRENT_USER=$(whoami)
sudo sh -c 'sed "s/\[user-name-here\]/'"$CURRENT_USER"'/g;
              s/\[branch-here\]/'"$BRANCH"'/g" \
            '"$REPO_ROOT"'/scripts/systemd/serial-keel.template.service > /etc/systemd/system/serial-keel.service'
sudo systemctl daemon-reload

echo "STEP 2/4: Stopping serial-keel server if already running.."
sudo systemctl stop serial-keel || true

echo "STEP 3/4: Installing the new serial-keel build.."
cargo install --bin serial-keel --path $REPO_ROOT/core --features mocks-share-endpoints

echo "STEP 4/4: Starting the serial-keel service"
sudo systemctl restart serial-keel
