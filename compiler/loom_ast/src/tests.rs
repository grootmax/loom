use loom_span::FileId;

use crate::{AstNode, Expr, Item, Pattern, SourceFile, Stmt, StringPart, Type};

fn file(source: &str) -> SourceFile {
    let parse = loom_parser::parse(source, FileId::DETACHED);
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics());
    SourceFile::cast(parse.syntax()).unwrap()
}

#[test]
fn items_and_signatures() {
    let source =
        "## Adds.\n@[inline]\npub def add[T: Num](a: T, own b: T = 1) -> T where T: Copy = a + b\n";
    let item = file(source).items().next().unwrap();
    assert_eq!(item.doc_comment().as_deref(), Some("Adds."));
    assert_eq!(item.attributes().count(), 1);
    assert!(item.visibility().is_some());
    let Item::FnDef(function) = item else {
        panic!()
    };
    assert_eq!(function.name().unwrap().text(), "add");
    let params: Vec<_> = function.param_list().unwrap().params().collect();
    assert_eq!(params.len(), 2);
    assert_eq!(params[1].mode().unwrap().text(), "own");
    assert!(params[1].default_value().is_some());
    assert!(matches!(function.ret_type(), Some(Type::PathType(_))));
    assert_eq!(function.generic_params().unwrap().params().count(), 1);
    assert!(function.where_clause().is_some());
    assert!(matches!(function.expr_body(), Some(Expr::BinaryExpr(_))));
}

#[test]
fn structs_enums_traits_impls() {
    let source = "struct P\n  x: Int\n  def self.new -> P = P(x: 0)\nend\n\
                  enum E\n  A\n  B(Int)\n  C(x: Int)\nend\n\
                  trait T\n  def m -> Int\n  default def n -> Int = 1\n  type Item\nend\n\
                  impl T for P\n  def m -> Int = 2\nend\n";
    let items: Vec<_> = file(source).items().collect();
    let Item::StructDef(structure) = &items[0] else {
        panic!()
    };
    assert_eq!(structure.fields().count(), 1);
    assert!(structure.methods().next().unwrap().is_associated());
    let Item::EnumDef(enumeration) = &items[1] else {
        panic!()
    };
    let variants: Vec<_> = enumeration.variants().collect();
    assert_eq!(variants.len(), 3);
    assert_eq!(variants[1].types().count(), 1);
    assert_eq!(variants[2].fields().count(), 1);
    let Item::TraitDef(tr) = &items[2] else {
        panic!()
    };
    let methods: Vec<_> = tr.methods().collect();
    assert!(!methods[0].is_default() && methods[1].is_default());
    assert_eq!(tr.assoc_types().count(), 1);
    let Item::ImplDef(implementation) = &items[3] else {
        panic!()
    };
    assert_eq!(implementation.trait_().unwrap().path().unwrap().text(), "T");
    assert!(implementation.self_type().is_some());
}

#[test]
fn statements_and_expressions() {
    let source = "def main\n  var x: Int = 1\n  x += 2\n  puts \"v=#{x}\"\n  case x\n  in 1 | 2 if x > 0 then 3\n  else 4\n  end\nend\n";
    let Item::FnDef(main) = file(source).items().next().unwrap() else {
        panic!()
    };
    let statements: Vec<_> = main.block_body().unwrap().statements().collect();
    let Stmt::BindStmt(binding) = &statements[0] else {
        panic!()
    };
    assert!(binding.is_var());
    assert!(binding.ty().is_some());
    let Stmt::AssignStmt(assign) = &statements[1] else {
        panic!()
    };
    assert_eq!(assign.operator().unwrap().text(), "+=");
    let Stmt::ExprStmt(command) = &statements[2] else {
        panic!()
    };
    let Some(Expr::CallExpr(call)) = command.expr() else {
        panic!()
    };
    assert!(call.is_command());
    let arg = call.arg_list().unwrap().args().next().unwrap();
    let crate::Argument::Arg(arg) = arg else {
        panic!()
    };
    let Some(Expr::StringLit(string)) = arg.value() else {
        panic!()
    };
    let parts: Vec<_> = string.parts().collect();
    assert!(matches!(parts[1], StringPart::Interpolation(_)));
    let Some(Expr::CaseExpr(case)) = main.block_body().unwrap().tail_expr() else {
        panic!()
    };
    let arm = case.arms().next().unwrap();
    assert!(matches!(arm.pattern(), Some(Pattern::OrPat(_))));
    assert!(arm.guard().is_some());
    assert!(case.else_branch().is_some());
}

#[test]
fn every_example_casts_to_items() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    for entry in std::fs::read_dir(examples).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "loom") {
            let source = std::fs::read_to_string(&path).unwrap();
            let source_file = file(&source);
            assert!(source_file.items().count() > 0, "{}", path.display());
            // Every item kind child of the file is an Item.
            for node in source_file.syntax().children() {
                if node.kind() != loom_syntax::SyntaxKind::ERROR {
                    assert!(
                        Item::cast(node.clone()).is_some(),
                        "{:?} in {}",
                        node,
                        path.display()
                    );
                }
            }
        }
    }
}
