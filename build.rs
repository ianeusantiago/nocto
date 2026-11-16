fn main() {
    // Embute o runtime do Visual C++ no exe: abre num Windows 10 recém-instalado,
    // sem pedir para instalar o "Visual C++ Redistributable".
    static_vcruntime::metabuild();
    // Ícone do Nocto no exe (aparece no Explorador, atalhos e Gerenciador de Tarefas).
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/nocto.ico");
    res.compile().expect("ícone");
    slint_build::compile("ui/island.slint").unwrap();
}
