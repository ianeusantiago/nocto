fn main() {
    // Embute o runtime do Visual C++ no exe: abre num Windows 10 recém-instalado,
    // sem pedir para instalar o "Visual C++ Redistributable".
    static_vcruntime::metabuild();
    slint_build::compile("ui/island.slint").unwrap();
}
