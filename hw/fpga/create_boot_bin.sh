#!/bin/bash
# Licensed under the Apache-2.0 license

# This script generates a Versal BOOT.BIN using Petalinux.
# When using an ubuntu image BOOT.BIN replaces boot1901.bin in the boot partition.

set -eu

scp /tmp/caliptra-fpga-bitstream/caliptra_fpga.xsa .
sudo chmod 755 caliptra_fpga.xsa
xsa_location=$(realpath $PWD/caliptra_fpga.xsa)

scp -r /fpga-tools/petalinux-tools .
sudo chmod -R 755 petalinux-tools
source petalinux-tools/settings.sh

echo Creating project
petalinux-create -t project --template versal --name petalinux_project
pushd petalinux_project
echo Adding xsa
petalinux-config --get-hw-description $xsa_location --silentconfig

echo Modifying Petalinux configuration
# Set ROOTFS to EXT4
sed -i 's|CONFIG_SUBSYSTEM_ROOTFS_INITRD=y|# CONFIG_SUBSYSTEM_ROOTFS_INITRD is not set|g' project-spec/configs/config
sed -i 's|# CONFIG_SUBSYSTEM_ROOTFS_EXT4 is not set|CONFIG_SUBSYSTEM_ROOTFS_EXT4=y|g' project-spec/configs/config
sed -i 's|CONFIG_SUBSYSTEM_INITRD_RAMDISK_LOADADDR=0x0|CONFIG_SUBSYSTEM_SDROOT_DEV="/dev/mmcblk0p2"|g' project-spec/configs/config
sed -i 's|CONFIG_SUBSYSTEM_INITRAMFS_IMAGE_NAME="petalinux-image-minimal"||g' project-spec/configs/config
sed -i 's|root=/dev/ram0 rw|root=/dev/mmcblk0p2 rw rootwait|g' project-spec/configs/config

echo Building FW components, only device-tree depends on XSA
petalinux-build -c device-tree
petalinux-build -c u-boot
petalinux-build -c arm-trusted-firmware
petalinux-build -c plm
petalinux-build -c psmfw

echo Modify device tree for 2024.2
dtc -I dtb -O dts -o images/linux/system.dts images/linux/system.dtb
sed -i 's/primecell/sbsa-uart/g' images/linux/system.dts
dtc -I dts -O dtb -o images/linux/system.dtb images/linux/system.dts

echo Packaging boot files
petalinux-package --boot --format BIN --plm --psmfw --u-boot --dtb --force
popd


(cat "${GITHUB_WORKSPACE}/hw/fpga/petalinux_project/build/config.log" || true)
