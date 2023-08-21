fn main() {
    // read env variables that were set in build script
    let uefi_path = env!("UEFI_PATH");
    let bios_path = env!("BIOS_PATH");

    println!("{}", uefi_path);

    // choose whether to start the UEFI or BIOS image
    let uefi = true;

    let mut cmd = std::process::Command::new("qemu-system-x86_64");
    if uefi {
        cmd.arg("--enable-kvm");
        cmd.arg("-nodefaults");

        cmd.arg("-m").arg("600M");
        // cmd.arg("-M").arg("q35");
        // cmd.arg("-device").arg("qemu-xhci");
        // cmd.arg("-device").arg("ahci");

        cmd.arg("-smp").arg("2");
        //GDB OPTS:
        // cmd.arg("-S").arg("-s");
        cmd.arg("-device").arg("virtio-mouse-pci");
        cmd.arg("-device").arg("virtio-keyboard-pci");
        cmd.arg("-nic").arg("user,model=virtio-net-pci");

        // cmd.arg("-monitor").arg("stdio");

        // cmd.arg("-device").arg("VGA,vgamem_mb=8");
        // cmd.arg("-device").arg("virtio-vga"); //gl
        // on linux guest cmd.arg("-display").arg("gtk,gl=on");
        // cmd.arg("-device").arg("virtio-gpu");
        cmd.arg("-device").arg("virtio-vga-gl");
        cmd.arg("-display").arg("sdl,gl=on");

        // cmd.arg("-vga").arg("none");

        // cmd.arg("-device").arg("bochs-display");
        // cmd.arg("-device").arg("qxl-vga");

        // cmd.arg("-nic").arg("none");

        cmd.arg("-serial").arg("stdio");

        cmd.arg("-pflash").arg("./ovmf");
        cmd.arg("-drive")
            .arg(format!("format=raw,file={uefi_path}"));
    } else {
        cmd.arg("-drive")
            .arg(format!("format=raw,file={bios_path}"));
    }
    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
