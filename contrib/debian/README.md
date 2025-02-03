## bpfman Installation from a PPA repository

Pre-reqs

```shell
sudo apt install devscripts debhelper-compat
```
To install the bpfman binaries using apt package management, first add the [bpfman PPA](https://launchpad.net/~bpfman) repository and perform an apt-get update. The supported hardware architectures are AMD64 and ARM64.

```shell
sudo add-apt-repository ppa:bpfman/ppa
sudo apt update
```

Next, install the bpfman package.

```shell
sudo apt-get install bpfman
```

The necessary systemd service files are installed but not enabled. To enable and start the service, run the following.

```shell
sudo systemctl enable bpfman
sudo systemctl start bpfman
```

To remove all binaries and files use the dpkg command and reference the service.

```shell
sudo apt-get purge bpfman
```

## Building and Installing a bpfman deb file

Since the debian builder expects the debian directory in the root of the source tree, first add a symbolic link in the bpfman directory.

```shell
ln -s contrib/debian debian
```

First build the `.deb` package.

```shell
cd bpfman/contrib
debuild -us -uc
```

The deb installer package will be located in the directory above the root directory of the bpfman source tree.

```text
sudo dpkg -i ../bpfman_<version>_amd64.deb
(Reading database ... 125420 files and directories currently installed.)
Preparing to unpack ../bpfman_<version>_amd64.deb ...
Unpacking bpfman (<version>) ...
Setting up bpfman (<version>) ...
Created symlink /etc/systemd/system/sockets.target.wants/bpfman.socket → /lib/systemd/system/bpfman.socket.
bpfman.service is a disabled or a static unit, not starting it.
```

To remove all binaries and files use the dpkg command and reference the service.

```shell
sudo dpkg --purge bpfman
```

## Maintaining the Bpfman PPA Packaging

The following is information regarding maintenance of the PPA packaging for maintainers.

First, generate a gpg key.

```shell
gpg --full-generate-key
gpg --list-keys
```

Next, authorize the keys on launchpad by uploading them to their key server and then authenticate with the gig key fingerprint and a decode message sent to the email on the key.

```shell
gpg --send-keys --keyserver keyserver.ubuntu.com KEY_ID
gpg --fingerprint
```

Alternatively, import the existing key associated to the `contact@bpfman.io` email and `bpfman.io` account.

```text
gpg --import <KEY_FILE>
```

Once you have the gpg key on the PPA repo installed locally, build the package to upload to the repo. Start by adding a symbolic link for the debian directory to point to the `contrib/debian` directory from the root of the bpfman directory.

```shell
ln -s contrib/debian debian
```

Build the package with `debuild`

```shell
debuild -us -uc -S
```

Sign the changes with the email account corresponding to the local gpg key. You will be prompted for the gpg passphrase for the key associated to the email and ID.

```shell
debsign -k <KEY_EMAIL> bpfman_<VERION>_source.changes
```

Push the changes to launchpad.

```shell
dput -f ppa:bpfman/ppa ./bpfman_<VERSION>_source.changes
```

You can view the status of the build on the [bpfman PPA](https://launchpad.net/~bpfman/+archive/ubuntu/ppa/+packages) page. Once changes are pushed and successfully build for the particular hardware arch, the new binaries can take up to an hour to get published. Until they are published, they will be in a `Pending publication` state. Deleted builds are garbage collected on a 6-hour cron job. This means there is a delay in the available capacity for the PPA until the deleted builds have been purged.
