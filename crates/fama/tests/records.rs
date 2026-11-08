use maslang::temp_directory::TemporaryDirectory;
use std::process::Command;

/// 经 fama new/build/run 的真实工程入口验证结构体列表及可空字段，不依赖用户全局编译器配置。
#[test]
fn project_build_and_run_support_records() {
    let root = TemporaryDirectory::create("fama-records").unwrap();
    let home = root.path().join("home");
    let binary = env!("CARGO_BIN_EXE_fama");
    let created = Command::new(binary)
        .args(["new", "demo"])
        .current_dir(root.path())
        .env("FAMA_HOME", &home)
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");
    let project = root.path().join("demo");
    std::fs::write(
        project.join("src/main.ms"),
        "struct Person:\n    age: int?\nvar p = Person(null)\nfunc main():\n    p.age = 18\n    var people: list<Person?> = []\n    people.append(p)\n    var named: map<string,Person?> = {\"p\": people[0]}\n    print(named[\"p\"].value.age.value)\n",
    )
    .unwrap();
    let built = Command::new(binary)
        .arg("build")
        .current_dir(&project)
        .env("FAMA_HOME", &home)
        .output()
        .unwrap();
    assert!(built.status.success(), "{built:?}");
    let output = Command::new(project.join(format!("build/demo{}", std::env::consts::EXE_SUFFIX)))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "18\n");
    let run = Command::new(binary)
        .arg("run")
        .current_dir(&project)
        .env("FAMA_HOME", &home)
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    assert!(String::from_utf8(run.stdout).unwrap().contains("18"));
}
