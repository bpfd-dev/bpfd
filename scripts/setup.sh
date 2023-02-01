#!/bin/bash

CALL_POPD=false
if [[ "$PWD" != */scripts ]]; then
    pushd scripts &>/dev/null
fi

# Source the functions in other files
. certificates.sh
. install.sh
. user.sh

USER_BPFD="bpfd"
USER_GROUP="bpfd"
BIN_BPFD="bpfd"
BIN_BPFCTL="bpfctl"
BIN_GOCOUNTER="gocounter"

# Well known directories
SRC_BIN_PATH="../target/debug"
DST_BIN_PATH="/usr/sbin"
DST_SVC_PATH="/usr/lib/systemd/system"
SRC_KUBECTL_PLUGIN_PATH="../bpfd-operator/hack"
DST_KUBECTL_PLUGIN_PATH="/usr/local/bin"

# ConfigurationDirectory: /etc/bpfd/
CONFIGURATION_DIR="/etc/bpfd"
CFG_CA_CERT_DIR="/etc/bpfd/certs/ca"

# RuntimeDirectory: /run/bpfd/
RUNTIME_DIR="/run/bpfd"
RTDIR_FS="/run/bpfd/fs"

# StateDirectory: /var/lib/bpfd/
STATE_DIR="/var/lib/bpfd"


usage() {
    echo "USAGE:"
    echo "sudo ./scripts/setup.sh install"
    echo "    Setup for running \"bpfd\" as a systemd service. Performs the following"
    echo "    tasks:"
    echo "    * Create User \"${USER_BPFD}\" and User Group \"${USER_GROUP}\"."
    echo "    * Copy \"bpfd\" and \"bpfctl\" binaries to \"/usr/sbin/.\" and set"
    echo "      the user group for each."
    echo "    * Copy \"bpfd.service\" to \"/usr/lib/systemd/system/\"."
    echo "    * Use \"systemctl\" to mange the service (the installer starts bpfd.service by default):"
    echo "          sudo systemctl start bpfd.service"
    echo "          sudo systemctl stop bpfd.service"
    echo "sudo ./scripts/setup.sh reinstall"
    echo "    Only copy the \"bpfd\" and \"bpfctl\" binaries to \"/usr/sbin/.\""
    echo "    and set the user group for each. \"bpfd\" service will be restarted"
    echo "    if running. Installed programs will need to be loaded again."
    echo "sudo ./scripts/setup.sh uninstall"
    echo "    Unwind all actions performed by \"setup.sh install\" including stopping"
    echo "    the \"bpfd\" service if it is running."
    echo "sudo ./scripts/setup.sh kubectl"
    echo "    Install kubectl plugins for \"bpfprogramconfigs\" and \"bpfprograms\"."
    echo ""
}

if [ $USER != "root" ]; then
    echo "ERROR: \"root\" or \"sudo\" required."
    exit
fi

case "$1" in
    "install")
        user_init
        install false
        ;;
    "reinstall")
        install true
        ;;
    "uninstall")
        uninstall
        user_del
        del_kubectl_plugin
        ;;
    "gocounter")
        cert_client gocounter ${USER_BPFD} false
        ;;
    "kubectl")
        copy_kubectl_plugin
        ;;
    "help"|"--help"|"?")
        usage
        ;;
    *)
        echo "Unknown input: $1"
        echo
        usage
        ;;
esac

if [[ "$CALL_POPD" == true ]]; then
    popd &>/dev/null
fi
