//! This is the type checker for the Vulpi language. It is responsible for checking the types of
//! the abstract syntax tree and producing a better output with semantic information that is the
//! elaborated tree. The type checker of Vulpi is based on the bidirectional type checking with
//! higher rank polymorphism and higher kinded types.

use std::collections::HashSet;

use env::Env;
use module::TypeData;
use vulpi_intern::Symbol;
use vulpi_syntax::r#abstract::*;

pub mod apply;
pub mod check;
pub mod env;
pub mod error;
pub mod infer;
pub mod kind;
pub mod module;
pub mod types;

pub use infer::types::*;
pub use infer::Infer;

pub trait Declare {
    fn declare(&self, context: Env);
    fn define(&self, _context: Env) {}
}

impl Declare for TypeDecl {
    fn declare(&self, mut context: Env) {
        let mut kinds = Vec::new();

        for binder in &self.binders {
            let (name, kind) = match binder {
                TypeBinder::Implicit(p) => (p.clone(), kind::Kind::star()),
                TypeBinder::Explicit(p, k) => (p.clone(), k.infer(())),
            };

            context.types.insert(name, kind.clone());
            kinds.push(kind);
        }

        let arrow = kinds
            .into_iter()
            .rfold(kind::Kind::star(), |acc, x| kind::Kind::arrow(x, acc));

        context
            .modules
            .borrow_mut()
            .get(self.name.path.clone())
            .types
            .insert(
                self.name.name.clone(),
                TypeData {
                    kind: arrow,
                    module: self.namespace.clone(),
                },
            );
    }

    fn define(&self, mut context: Env) {
        let mut kinds = Vec::new();
        let mut types = Vec::new();

        for binder in &self.binders {
            let (name, kind) = match binder {
                TypeBinder::Implicit(p) => (p.clone(), kind::Kind::star()),
                TypeBinder::Explicit(p, k) => (p.clone(), k.infer(())),
            };

            context.types.insert(name.clone(), kind.clone());

            kinds.push((name.clone(), kind));
            types.push(types::Type::named(name));
        }

        let ret = types::Type::app(types::Type::variable(self.name.clone()), types);

        match &self.def {
            TypeDef::Sum(sum) => {
                for cons in &sum.constructors {
                    let types = cons.args.iter().map(|x| {
                        let (t, k) = x.infer(&context);
                        k.unify(&context, &kind::Kind::star());
                        t
                    });

                    let ret = if let Some(new_ret) = &cons.typ {
                        let (t, k) = new_ret.infer(&context);
                        k.unify(&context, &kind::Kind::star());

                        let foralled = kinds
                            .clone()
                            .into_iter()
                            .rfold(ret.clone(), |acc, (n, k)| types::Type::forall(n, k, acc));

                        types::Type::sub(&foralled, context.clone(), t.clone());

                        t
                    } else {
                        ret.clone()
                    };

                    let typ = types.rfold(ret.clone(), |acc, x| types::Type::arrow(x, acc));

                    let typ = kinds
                        .clone()
                        .into_iter()
                        .rfold(typ.clone(), |acc, (n, k)| types::Type::forall(n, k, acc));

                    context
                        .modules
                        .borrow_mut()
                        .get(self.namespace.clone())
                        .constructors
                        .insert(cons.name.clone(), (typ, cons.args.len()));
                }
            }
            TypeDef::Record(rec) => {
                for field in &rec.fields {
                    let (t, k) = field.1.infer(&context);

                    let typ = kinds
                        .clone()
                        .into_iter()
                        .rfold(t.clone(), |acc, (n, k)| types::Type::forall(n, k, acc));

                    k.unify(&context, &kind::Kind::star());

                    context
                        .modules
                        .borrow_mut()
                        .get(context.current_namespace())
                        .fields
                        .insert(field.0.clone(), typ);
                }
            }
            TypeDef::Synonym(_) => todo!(),
            TypeDef::Abstract => (),
        }
    }
}

impl Declare for EffectDecl {
    fn declare(&self, mut context: Env) {
        let mut kinds = Vec::new();

        for binder in &self.binders {
            let (name, kind) = match binder {
                TypeBinder::Implicit(p) => (p.clone(), kind::Kind::star()),
                TypeBinder::Explicit(p, k) => (p.clone(), k.infer(())),
            };

            context.types.insert(name, kind.clone());
            kinds.push(kind);
        }

        let arrow = kinds
            .into_iter()
            .rfold(kind::Kind::var(Symbol::intern("Effect")), |acc, x| {
                kind::Kind::arrow(x, acc)
            });

        context
            .modules
            .borrow_mut()
            .get(self.qualified.path.clone())
            .types
            .insert(
                self.qualified.name.clone(),
                TypeData {
                    kind: arrow,
                    module: self.namespace.clone(),
                },
            );
    }

    fn define(&self, mut context: Env) {
        let mut kinds = Vec::new();
        let mut types = Vec::new();

        for binder in &self.binders {
            let (name, kind) = match binder {
                TypeBinder::Implicit(p) => (p.clone(), kind::Kind::star()),
                TypeBinder::Explicit(p, k) => (p.clone(), k.infer(())),
            };

            context.types.insert(name.clone(), kind.clone());

            kinds.push((name.clone(), kind));
            types.push(types::Type::named(name));
        }

        for eff in &self.fields {
            let types = eff.args.iter().map(|x| {
                let (t, k) = x.infer(&context);
                k.unify(&context, &kind::Kind::star());
                t
            });

            let (init, k) = eff.ty.infer(&context);
            k.unify(&context, &kind::Kind::star());

            let typ = types.rfold(init, |acc, x| types::Type::arrow(x, acc));

            let typ = kinds
                .clone()
                .into_iter()
                .rfold(typ.clone(), |acc, (n, k)| types::Type::forall(n, k, acc));

            context
                .modules
                .borrow_mut()
                .get(context.current_namespace())
                .effects
                .insert(eff.name.clone(), typ);
        }
    }
}

impl Declare for LetDecl {
    fn declare(&self, mut context: Env) {
        let fvs = self
            .binders
            .iter()
            .map(|x| x.ty.data.free_variables())
            .fold(HashSet::new(), |acc, x| acc.union(&x).cloned().collect());

        for fv in &fvs {
            context.types.insert(fv.clone(), kind::Kind::star());
        }

        let ret = match &self.ret {
            Some((_, t)) => {
                let (t, k) = t.infer(&context);
                k.unify(&context, &kind::Kind::star());
                t
            }
            None => context.new_hole(),
        };

        let typ = self
            .binders
            .iter()
            .map(|x| {
                let (t, k) = x.ty.infer(&context);
                k.unify(&context, &kind::Kind::star());
                t
            })
            .rfold(ret, |acc, x| types::Type::arrow(x, acc));

        let typ = fvs.into_iter().fold(typ, |acc, x| {
            types::Type::forall(x, kind::Kind::star(), acc)
        });

        context
            .modules
            .borrow_mut()
            .get(context.current_namespace())
            .variables
            .insert(self.name.clone(), typ);
    }

    fn define(&self, context: Env) {
        self.infer(context);
    }
}

impl Declare for ModuleDecl {
    fn declare(&self, mut context: Env) {
        context.on(self.namespace.clone(), |context| {
            if let Some(types) = self.types() {
                for decl in types {
                    decl.declare(context.clone());
                }
            }

            if let Some(effs) = self.effects() {
                for decl in effs {
                    decl.declare(context.clone());
                }
            }

            if let Some(modules) = self.modules() {
                for decl in modules {
                    decl.declare(context.clone());
                }
            }

            if let Some(lets) = self.lets() {
                for decl in lets {
                    decl.declare(context.clone());
                }
            }
        })
    }

    fn define(&self, mut context: Env) {
        context.on(self.namespace.clone(), |context| {
            if let Some(types) = self.types() {
                for decl in types {
                    decl.define(context.clone());
                }
            }

            if let Some(effs) = self.effects() {
                for decl in effs {
                    decl.define(context.clone());
                }
            }

            if let Some(modules) = self.modules() {
                for decl in modules {
                    decl.define(context.clone());
                }
            }

            if let Some(lets) = self.lets() {
                for decl in lets {
                    decl.define(context.clone());
                }
            }
        })
    }
}

impl Declare for Module {
    fn declare(&self, context: Env) {
        for decl in self.types() {
            decl.declare(context.clone());
        }

        for effs in self.effects() {
            effs.declare(context.clone());
        }

        for modules in self.modules() {
            modules.declare(context.clone());
        }

        for del in self.lets() {
            del.declare(context.clone());
        }
    }

    fn define(&self, context: Env) {
        for module in self.modules() {
            module.define(context.clone());
        }

        for decl in self.types() {
            decl.define(context.clone());
        }

        for effs in self.effects() {
            effs.define(context.clone());
        }

        for lets in self.lets() {
            lets.define(context.clone());
        }
    }
}
