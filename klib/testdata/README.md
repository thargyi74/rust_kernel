# klib test data

## `qemu-virt-512m.dtb`

The device tree that QEMU's `virt` machine gives the kernel, for the
`klib::fdt` tests. It was made with QEMU 11.1.3 and DTC 1.8.1:

```sh
S=$(mktemp -d)
qemu-system-aarch64 -machine virt,dumpdtb=$S/virt.dtb -cpu cortex-a72 -m 512M -smp 1 -nographic
dtc -I dtb -O dtb -o klib/testdata/qemu-virt-512m.dtb $S/virt.dtb
```

QEMU pads its dump to 1 MiB. The second command recompiles it without the
padding, so the file is a few KiB. Check it with:

```sh
dtc -I dtb -O dts klib/testdata/qemu-virt-512m.dtb | grep -A2 memory@
```

It shows `reg = <0x00 0x40000000 0x00 0x20000000>`: 512 MiB of RAM at 1 GiB.
The `rng-seed` and `kaslr-seed` values in `/chosen` are random for each dump.
