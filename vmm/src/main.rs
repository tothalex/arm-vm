mod vmm;

fn main() {
    let vm = vmm::Vm::new(512);

    vm.configure();
}
