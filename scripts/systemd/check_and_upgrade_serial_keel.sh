#!/bin/bash
# This script should only be triggered by the serial-keel service (as a pre-execution script)
# to upgrade the serial-keel server if an update is available on the branch specified
set -e


# The name of the branch can be specified as a argument, or it defaults to 'main'
BRANCH=${1-main}
CONFIG_PATH=${2-$HOME/config.ron}

SCRIPT_PATH="$( cd -- "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"
cd $SCRIPT_PATH

# If we happen to run this script for the first time, then we call the install script
if ! test -f "$HOME/.config/systemd/user/serial-keel.service"; then
    echo "We happen to be running the upgrade script before we've had a first time install.."
    ./install_serial_keel.sh $BRANCH $CONFIG_PATH
    exit 0
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
cd $REPO_ROOT

# Check if there are new changes and upgrade serial-keel if necessary
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


if $GIT_NO_NEW_REMOTE_CHANGES && $GIT_NO_BRANCH_SWITCH; then
    echo "serial-keel already up to date."
    exit 0
fi

echo "Upgrading serial-keel.."
cargo install --bin serial-keel --path $REPO_ROOT/core --features mocks-share-endpoints
