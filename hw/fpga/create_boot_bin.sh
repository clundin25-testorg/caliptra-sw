#!/bin/bash
# Licensed under the Apache-2.0 license

# This script generates a Versal BOOT.BIN using Petalinux.
# When using an ubuntu image BOOT.BIN replaces boot1901.bin in the boot partition.

# CALIPTRA_CI re-uses the checked in petalinux project.
if [[ -z $1 ]] && [[ -z ${CALIPTRA_CI} ]]; then
    echo "create_boot_bin.sh [/path/to/caliptra_fpga_project_bd_wrapper.xsa]"
    exit
fi

trap '{
  if [ $? -ne 0 ]
  then
    echo FAILED TO CREATE BOOT.BIN
    exit 1
  else
    echo SUCCESS
  fi  
}' EXIT

# Always create a fresh petalinux_project when run outside of CI.
if [[ -z ${CALIPTRA_CI} ]]; then
    xsa_location=$(realpath $1)

    set -ex
    (rm -rf petalinux_project || true)

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

    popd
else
    # Copy the latest VCK 190 bitstream from GCP bucket.
    gcloud storage cp gs://${CALIPTRA_CI_BUCKET}/vck190_bitstream.tar.gz .
    tar -C petalinux_project/project-spec/hw-description -xvf vck190_bitstream.tar.gz

    # Bring in petalinux toolchain.
    source /vck190-tools/petalinux-tool/settings.sh
fi

pushd petalinux_project

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
