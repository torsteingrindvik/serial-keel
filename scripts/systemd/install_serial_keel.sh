#!/bin/bash
# This script should only be triggered by an user manually to setup serial-keel for the first time,
# or if there were changes in the .service file
set -e

mkdir -p $HOME/.config/systemd/user/
mkdir -p $HOME/.config/environment.d/

# The name of the branch can be specified as a argument, or it defaults to 'main'
BRANCH=${1-main}
CONFIG_PATH=${2-$HOME/config.ron}
ENV_FILE=$HOME/.config/environment.d/serial-keel.conf

if ! test -f $ENV_FILE; then
    env > $ENV_FILE
fi

SCRIPT_PATH="$( cd -- "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"
cd $SCRIPT_PATH

REPO_ROOT=$(git rev-parse --show-toplevel)
cd $REPO_ROOT

echo "STEP 1/4: Copying the serial-keel.service file"

# Fill in the service file template and place it in ~/.config/systemd/user/
sed "s|ExecStartPre=.*|ExecStartPre=$REPO_ROOT\/scripts\/systemd\/check_and_upgrade_serial_keel.sh $BRANCH|;
     s|ExecStart=.*|ExecStart=$HOME\/.cargo\/bin\/serial-keel $CONFIG_PATH|" $REPO_ROOT/scripts/systemd/serial-keel.template.service > $HOME/.config/systemd/user/serial-keel.service

systemctl --user enable serial-keel.service
systemctl --user daemon-reload

echo "STEP 2/4: Stopping serial-keel server if already running.."
systemctl --user stop serial-keel || true

echo "STEP 3/4: Installing the new serial-keel build.."
cargo install --bin serial-keel --path $REPO_ROOT/core --features mocks-share-endpoints

echo "STEP 4/4: Starting the serial-keel service"
systemctl --user start serial-keel

# We also need to enable-linger for the service to start on system reboot and not just on user login
loginctl enable-linger
