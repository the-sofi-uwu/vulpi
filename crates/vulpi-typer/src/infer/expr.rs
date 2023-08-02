use crate::apply::Apply;
use im_rc::HashMap;
use vulpi_intern::Symbol;
use vulpi_syntax::{
    r#abstract::Expr, r#abstract::ExprKind, r#abstract::PatternArm, r#abstract::StatementKind,
};

use crate::{env::Env, types::Type, Infer};

impl Infer for PatternArm {
    type Return = (Vec<Type>, Type);

    type Context<'a> = Env;

    fn infer(&self, mut context: Self::Context<'_>) -> Self::Return {
        let mut tys = Vec::new();

        let mut bindings = HashMap::new();

        for pat in &self.patterns {
            tys.push(pat.infer((context.clone(), &mut bindings)))
        }

        for binding in bindings {
            context.add_variable(binding.0, binding.1 .1)
        }

        if let Some(ty) = self.guard.infer(context.clone()) {
            let right = context.imports.get(&Symbol::intern("Bool")).unwrap();
            let right = Type::variable(right.clone());

            Type::unify(context.clone(), ty, right);
        }

        let result = self.expr.infer(context);

        (tys, result)
    }
}

impl Infer for (usize, &Vec<&PatternArm>) {
    type Return = (Vec<Type>, Type);

    type Context<'a> = Env;

    fn infer(&self, context: Self::Context<'_>) -> Self::Return {
        let (size, arms) = self;
        let types = (0..*size).map(|_| context.new_hole()).collect::<Vec<_>>();
        let ret = context.new_hole();

        for arm in *arms {
            if arm.patterns.len() != *size {
                context.report(crate::error::TypeErrorKind::WrongArity(
                    *size,
                    arm.patterns.len(),
                ));
                return (Vec::new(), Type::error());
            }

            let (tys, ty) = arm.infer(context.clone());

            for (left, right) in types.iter().zip(tys.into_iter()) {
                Type::unify(context.clone(), left.clone(), right);
            }

            Type::unify(context.clone(), ret.clone(), ty);
        }

        (types, ret)
    }
}

impl Infer for Expr {
    type Return = Type;

    type Context<'a> = Env;

    fn infer(&self, mut context: Self::Context<'_>) -> Self::Return {
        context.set_location(self.span.clone());

        match &self.data {
            ExprKind::Lambda(lam) => {
                let mut bindings = HashMap::new();
                let ty = lam.param.infer((context.clone(), &mut bindings));

                for (k, (_, t)) in bindings {
                    context.add_variable(k, t)
                }

                let body = lam.body.infer(context);

                Type::arrow(ty, body)
            }
            ExprKind::Variable(var) => context.variables.get(var).unwrap().clone(),
            ExprKind::Constructor(cons) => context.get_module_constructor(cons).0,
            ExprKind::Function(name) => context.get_module_let(name),
            ExprKind::Let(let_) => {
                let mut bindings = HashMap::new();
                let ty = let_.pattern.infer((context.clone(), &mut bindings));
                let ty2 = let_.body.infer(context.clone());
                Type::unify(context.clone(), ty, ty2);

                for (k, (_, t)) in bindings {
                    context.add_variable(k, t)
                }

                let_.value.infer(context)
            }

            ExprKind::When(when) => {
                let scrutinee = when.scrutinee.infer(context.clone());

                let (types, ret) =
                    (1, &when.arms.iter().collect::<Vec<_>>()).infer(context.clone());

                if types.len() == 1 {
                    let ty = types.first().unwrap().clone();
                    Type::unify(context.clone(), scrutinee, ty);
                    ret
                } else {
                    Type::error()
                }
            }
            ExprKind::Do(not) => {
                let Some(unit_qual) = context.import("Unit") else {
                    return Type::error();
                };

                let unit = Type::variable(unit_qual);

                let mut res_ty = unit.clone();

                for expr in &not.statements {
                    context.set_location(expr.span.clone());
                    match &expr.data {
                        StatementKind::Let(let_) => {
                            let mut bindings = HashMap::new();
                            let ty = let_.pattern.infer((context.clone(), &mut bindings));

                            let body = let_.expr.infer(context.clone());
                            Type::unify(context.clone(), ty, body);

                            for (k, (_, t)) in bindings {
                                context.add_variable(k, t)
                            }

                            res_ty = unit.clone()
                        }
                        StatementKind::Expr(e) => {
                            res_ty = e.infer(context.clone());
                        }
                        StatementKind::Error => todo!(),
                    }
                }

                res_ty
            }
            ExprKind::Application(app) => {
                let mut ty = app.func.infer(context.clone());

                for arg in &app.args {
                    let ty2 = arg.apply(ty, context.clone());
                    ty = ty2;
                }

                ty
            }

            ExprKind::Literal(l) => l.infer(&context),
            ExprKind::Annotation(ann) => {
                let (ty, _) = ann.ty.infer(&context);
                let ty2 = ann.expr.infer(context.clone());
                Type::unify(context, ty, ty2.clone());
                ty2
            }

            ExprKind::Projection(_) => todo!(),
            ExprKind::RecordInstance(_) => todo!(),
            ExprKind::RecordUpdate(_) => todo!(),

            ExprKind::Tuple(tuple) => {
                let mut types = Vec::new();

                for expr in &tuple.exprs {
                    types.push(expr.infer(context.clone()));
                }

                Type::tuple(types)
            }
            ExprKind::Error => Type::error(),

            ExprKind::Handler(_) => todo!(),
            ExprKind::Cases(_) => todo!(),
            ExprKind::Effect(_) => todo!(),
        }
    }
}