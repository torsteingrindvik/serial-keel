#!/bin/bash
# This script will either setup a fresh serial-keel service instance, or upgrade your existing one.
# This will also be executed during every bootup, so a check for an upgrade will be performed.
set -e

CURRENT_USER=$(whoami)
USER_HOME="/home/$CURRENT_USER/"

mkdir -p $USER_HOME/.config/systemd/user/

# The name of the branch can be specified as a argument, or it defaults to 'main'
BRANCH=${1-main}
CONFIG_PATH=${2-/home/$CURRENT_USER/config.ron}

SCRIPT_PATH="$( cd -- "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"
cd $SCRIPT_PATH

REPO_ROOT=$(git rev-parse --show-toplevel)
cd $REPO_ROOT

# Check if there are new changes. If there are none, and if the serial-keel service
# already exists on the machine, then start the serial-keel server

GIT_NO_BRANCH_SWITCH=true
if [[ $(git rev-parse --abbrev-ref HEAD) != "$BRANCH" ]]; then
    GIT_NO_BRANCH_SWITCH=false
    git checkout $BRANCH
fi

git fetch

GIT_NO_NEW_REMOTE_CHANGES=true
if git status -uno | grep -q "Your branch is behind"; then
    GIT_NO_NEW_REMOTE_CHANGES=false
    git pull
fi


if $GIT_NO_NEW_REMOTE_CHANGES && $GIT_NO_BRANCH_SWITCH && test -f $USER_HOME/.config/systemd/user/serial-keel.service; then
    echo "serial-keel already up to date."
    exit 0
fi

# If not, we have a serial-keel update
echo "We have a serial-keel update to perform.."
mkdir -p $USER_HOME/.config/environment.d/
env > $USER_HOME/.config/environment.d/env

echo "STEP 1/4: Copying the serial-keel.service file, \
in case we have a fresh installation, or if there were updates"

# This replaces all occurences of [user-name-here] and [branch-here] in the serial-keel.template file and 
# with the current user and branch name and places the service file in /etc/systemd/system
sed "s|\[user-name-here\]|$CURRENT_USER|g;
     s|ExecStartPre=.*|ExecStartPre=$REPO_ROOT\/scripts\/systemd\/check_and_upgrade_serial_keel.sh $BRANCH|;
     s|ExecStart=.*|ExecStart=/home/$CURRENT_USER/.cargo/bin/serial-keel $CONFIG_PATH|" $REPO_ROOT/scripts/systemd/serial-keel.template.service > $USER_HOME/.config/systemd/user/serial-keel.service

systemctl --user enable serial-keel.service
systemctl --user daemon-reload

echo "STEP 2/4: Stopping serial-keel server if already running.."
systemctl --user stop serial-keel || true

echo "STEP 3/4: Installing the new serial-keel build.."
cargo install --bin serial-keel --path $REPO_ROOT/core --features mocks-share-endpoints

echo "STEP 4/4: Starting the serial-keel service"
systemctl --user start serial-keel
