#![feature(plugin_registrar)]
#![feature(macro_vis_matcher)]
#![feature(box_syntax, rustc_private)]

extern crate syntax;
extern crate syntax_pos;

// Load rustc as a plugin to get macros
#[macro_use]
extern crate rustc;
extern crate rustc_plugin;


use rustc::lint::{EarlyContext, LintContext, LintPass, EarlyLintPass, EarlyLintPassObject, LintArray};
use rustc_plugin::registry::Registry;
// use rustc::plugin::Registry;
use syntax::visit::FnKind;
use syntax::ast::{FnDecl, NodeId};
use syntax_pos::Span;


declare_lint!(pub APPLICATION_MAIN_FN, Deny, "Checks that application crates have a proper main fn signature");

struct Pass {
	found_main_fn: bool,
}
impl Pass {
	fn new() -> Pass {
		Pass { 
			found_main_fn: false,
		}
	}
}

impl LintPass for Pass {
	fn get_lints(&self) -> LintArray {
		lint_array!(APPLICATION_MAIN_FN)
	}
}

impl EarlyLintPass for Pass {
	fn check_fn(&mut self, ecx: &EarlyContext, kind: FnKind, decl: &FnDecl, span: Span, node_id: NodeId) {
		println!("{:?} {:?} {:?}", decl, span, node_id);
		if let FnKind::ItemFn(ident, unsafety, spanned, abi, visibility, block) = kind {
			println!("    {:?} {:?} {:?} {:?} {:?} ", ident, unsafety, spanned, abi, visibility);
		}

		// check inputs
		if let Some(first_arg) = decl.inputs.get(0) {
			println!("        first arg {:?}", first_arg);
			// first_arg.ty
		}
	}


	// fn check_item(&mut self, cx: &EarlyContext, it: &ast::Item) {
	// 	if it.ident.name.as_str() == "main" {
	// 		println!("{:?}", it);
	// 		let node = cx.get(it.node);

	// 	}
		
	// 	// cx.span_lint(APPLICATION_MAIN_FN, it.span, "item is named 'lintme'");
	// }
}

#[plugin_registrar]
pub fn plugin_registrar(reg: &mut Registry) {
	reg.register_early_lint_pass(box Pass::new() as EarlyLintPassObject);
}
