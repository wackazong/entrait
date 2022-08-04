//! # entrait_macros
//!
//! Procedural macros used by entrait.
//!

pub mod input_attr;

use crate::analyze_generics;
use crate::analyze_generics::GenericsAnalyzer;
use crate::analyze_generics::TraitFn;
use crate::attributes;
use crate::generics::{self, TraitDependencyMode};
use crate::idents::CrateIdents;
use crate::input::FnInputMode;
use crate::input::{InputFn, InputMod, ModItem};
use crate::opt::*;
use crate::signature;
use crate::token_util::{push_tokens, TokenPair};
use input_attr::*;

use proc_macro2::Span;
use proc_macro2::TokenStream;
use quote::quote_spanned;
use quote::{quote, ToTokens};

use crate::analyze_generics::detect_trait_dependency_mode;

pub fn entrait_for_single_fn(attr: &EntraitFnAttr, input_fn: InputFn) -> syn::Result<TokenStream> {
    let mode = FnInputMode::SingleFn(&input_fn.fn_sig.ident);
    let mut generics_analyzer = GenericsAnalyzer::new();
    let trait_fns = [TraitFn::analyze(
        &input_fn,
        &mut generics_analyzer,
        signature::FnIndex(0),
        attr.trait_ident.span(),
        &attr.opts,
    )?];

    let trait_dependency_mode = detect_trait_dependency_mode(
        &mode,
        &trait_fns,
        &attr.crate_idents,
        attr.trait_ident.span(),
    )?;
    let use_associated_future =
        generics::detect_use_associated_future(&attr.opts, [&input_fn].into_iter());

    let trait_generics = generics_analyzer.into_trait_generics();
    let trait_def = gen_trait_def(
        attr,
        &trait_generics,
        &trait_dependency_mode,
        &trait_fns,
        &mode,
    )?;
    let impl_block = gen_impl_block(
        attr,
        &trait_generics,
        &trait_dependency_mode,
        &trait_fns,
        use_associated_future,
    );

    let InputFn {
        fn_attrs,
        fn_vis,
        fn_sig,
        fn_body,
        ..
    } = input_fn;

    Ok(quote! {
        #(#fn_attrs)* #fn_vis #fn_sig #fn_body
        #trait_def
        #impl_block
    })
}

pub fn entrait_for_mod(attr: &EntraitFnAttr, input_mod: InputMod) -> syn::Result<TokenStream> {
    let mode = FnInputMode::Module;
    let mut generics_analyzer = analyze_generics::GenericsAnalyzer::new();
    let trait_fns = input_mod
        .items
        .iter()
        .filter_map(ModItem::filter_pub_fn)
        .enumerate()
        .map(|(index, input_fn)| {
            TraitFn::analyze(
                input_fn,
                &mut generics_analyzer,
                signature::FnIndex(index),
                attr.trait_ident.span(),
                &attr.opts,
            )
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let trait_dependency_mode = detect_trait_dependency_mode(
        &mode,
        &trait_fns,
        &attr.crate_idents,
        attr.trait_ident.span(),
    )?;
    let use_associated_future = generics::detect_use_associated_future(
        &attr.opts,
        input_mod.items.iter().filter_map(ModItem::filter_pub_fn),
    );

    let trait_generics = generics_analyzer.into_trait_generics();
    let trait_def = gen_trait_def(
        attr,
        &trait_generics,
        &trait_dependency_mode,
        &trait_fns,
        &mode,
    )?;
    let impl_block = gen_impl_block(
        attr,
        &trait_generics,
        &trait_dependency_mode,
        &trait_fns,
        use_associated_future,
    );

    let InputMod {
        attrs,
        vis,
        mod_token,
        ident: mod_ident,
        items,
        ..
    } = input_mod;

    let trait_vis = &attr.trait_visibility;
    let trait_ident = &attr.trait_ident;

    Ok(quote! {
        #(#attrs)*
        #vis #mod_token #mod_ident {
            #(#items)*

            #trait_def
            #impl_block
        }

        #trait_vis use #mod_ident::#trait_ident;
    })
}

fn gen_trait_def(
    attr: &EntraitFnAttr,
    trait_generics: &generics::TraitGenerics,
    trait_dependency_mode: &TraitDependencyMode,
    trait_fns: &[TraitFn],
    mode: &FnInputMode<'_>,
) -> syn::Result<TokenStream> {
    let span = attr.trait_ident.span();

    let opt_unimock_attr = match attr.opts.default_option(attr.opts.unimock, false) {
        SpanOpt(true, span) => Some(attributes::ExportGatedAttr {
            params: attributes::UnimockAttrParams {
                crate_idents: &attr.crate_idents,
                trait_fns,
                mode,
                span,
            },
            opts: &attr.opts,
        }),
        _ => None,
    };

    // let opt_unimock_attr = attr.opt_unimock_attribute(trait_fns, mode);
    let opt_entrait_for_trait_attr = match trait_dependency_mode {
        TraitDependencyMode::Concrete(_) => {
            Some(attributes::Attr(attributes::EntraitForTraitParams {
                crate_idents: &attr.crate_idents,
            }))
        }
        _ => None,
    };

    let opt_mockall_automock_attr = match attr.opts.default_option(attr.opts.mockall, false) {
        SpanOpt(true, span) => Some(attributes::ExportGatedAttr {
            params: attributes::MockallAutomockParams { span },
            opts: &attr.opts,
        }),
        _ => None,
    };
    let opt_async_trait_attr =
        opt_async_trait_attribute(&attr.opts, &attr.crate_idents, trait_fns.iter());

    let trait_visibility = TraitVisibility { attr, mode };
    let trait_ident = &attr.trait_ident;

    let fn_defs = trait_fns.iter().map(|trait_fn| {
        let opt_associated_fut_decl = &trait_fn.entrait_sig.associated_fut_decl;
        let trait_fn_sig = trait_fn.sig();

        quote! {
            #opt_associated_fut_decl
            #trait_fn_sig;
        }
    });

    let params = trait_generics.trait_params();
    let where_clause = trait_generics.trait_where_clause();

    Ok(quote_spanned! { span=>
        #opt_unimock_attr
        #opt_entrait_for_trait_attr
        #opt_mockall_automock_attr
        #opt_async_trait_attr
        #trait_visibility trait #trait_ident #params #where_clause {
            #(#fn_defs)*
        }
    })
}

struct TraitVisibility<'a> {
    attr: &'a EntraitFnAttr,
    mode: &'a FnInputMode<'a>,
}

impl<'a> ToTokens for TraitVisibility<'a> {
    fn to_tokens(&self, stream: &mut TokenStream) {
        match &self.mode {
            FnInputMode::Module => {
                match &self.attr.trait_visibility {
                    syn::Visibility::Inherited => {
                        // When the trait is "private", it should only be accessible to the module outside,
                        // so use `pub(super)`.
                        // This is because the trait is syntacitally "defined" outside the module, because
                        // the attribute is an outer attribute.
                        // If proc-macros supported inner attributes, and this was invoked with that, we wouldn't do this.
                        push_tokens!(stream, syn::token::Pub(Span::call_site()));
                        syn::token::Paren::default().surround(stream, |stream| {
                            push_tokens!(stream, syn::token::Super::default());
                        });
                    }
                    _ => {
                        push_tokens!(stream, self.attr.trait_visibility);
                    }
                }
            }
            FnInputMode::SingleFn(_) => {
                push_tokens!(stream, self.attr.trait_visibility);
            }
        }
    }
}

///
/// Generate code like
///
/// ```no_compile
/// impl<__T: ::entrait::Impl + Deps> Trait for __T {
///     fn the_func(&self, args...) {
///         the_func(self, args)
///     }
/// }
/// ```
///
fn gen_impl_block(
    attr: &EntraitFnAttr,
    trait_generics: &generics::TraitGenerics,
    trait_dependency_mode: &TraitDependencyMode,
    trait_fns: &[TraitFn],
    use_associated_future: generics::UseAssociatedFuture,
) -> TokenStream {
    let span = attr.trait_ident.span();

    let async_trait_attribute =
        opt_async_trait_attribute(&attr.opts, &attr.crate_idents, trait_fns.iter());
    let params = trait_generics.impl_params(trait_dependency_mode, use_associated_future);
    let trait_ident = &attr.trait_ident;
    let args = trait_generics.arguments();
    let self_ty = SelfTy(trait_dependency_mode, span);
    let where_clause = trait_generics.impl_where_clause(trait_fns, trait_dependency_mode, span);

    let items = trait_fns.iter().map(|trait_fn| {
        let associated_fut_impl = &trait_fn.entrait_sig.associated_fut_impl;

        let fn_item = gen_delegating_fn_item(trait_fn, span);

        quote! {
            #associated_fut_impl
            #fn_item
        }
    });

    quote_spanned! { span=>
        #async_trait_attribute
        impl #params #trait_ident #args for #self_ty #where_clause {
            #(#items)*
        }
    }
}

struct SelfTy<'g, 'c>(&'g TraitDependencyMode<'g, 'c>, Span);

impl<'g, 'c> quote::ToTokens for SelfTy<'g, 'c> {
    fn to_tokens(&self, stream: &mut TokenStream) {
        let span = self.1;
        match &self.0 {
            TraitDependencyMode::Generic(idents) => {
                push_tokens!(stream, idents.impl_path(span))
            }
            TraitDependencyMode::Concrete(ty) => {
                push_tokens!(stream, ty)
            }
        }
    }
}

/// Generate the fn (in the impl block) that calls the entraited fn
fn gen_delegating_fn_item(trait_fn: &TraitFn, span: Span) -> TokenStream {
    let entrait_sig = &trait_fn.entrait_sig;
    let trait_fn_sig = &trait_fn.sig();
    let deps = &trait_fn.deps;

    let mut fn_ident = trait_fn.source.fn_sig.ident.clone();
    fn_ident.set_span(span);

    let opt_self_comma = match (deps, entrait_sig.sig.inputs.first()) {
        (generics::FnDeps::NoDeps { .. }, _) | (_, None) => None,
        (_, Some(_)) => Some(TokenPair(
            syn::token::SelfValue(span),
            syn::token::Comma(span),
        )),
    };

    let arguments = entrait_sig
        .sig
        .inputs
        .iter()
        .filter_map(|fn_arg| match fn_arg {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
                syn::Pat::Ident(pat_ident) => Some(&pat_ident.ident),
                _ => panic!("Found a non-ident pattern, this should be handled in signature.rs"),
            },
        });

    let mut opt_dot_await = trait_fn.source.opt_dot_await(span);
    if entrait_sig.associated_fut_decl.is_some() {
        opt_dot_await = None;
    }

    quote_spanned! { span=>
        #trait_fn_sig {
            #fn_ident(#opt_self_comma #(#arguments),*) #opt_dot_await
        }
    }
}

impl InputFn {
    fn opt_dot_await(&self, span: Span) -> Option<impl ToTokens> {
        if self.fn_sig.asyncness.is_some() {
            Some(TokenPair(syn::token::Dot(span), syn::token::Await(span)))
        } else {
            None
        }
    }

    pub fn use_associated_future(&self, opts: &Opts) -> bool {
        matches!(
            (opts.async_strategy(), self.fn_sig.asyncness),
            (SpanOpt(AsyncStrategy::AssociatedFuture, _), Some(_async))
        )
    }
}

fn opt_async_trait_attribute<'s, 'o>(
    opts: &'s Opts,
    crate_idents: &'s CrateIdents,
    trait_fns: impl Iterator<Item = &'o TraitFn<'o>>,
) -> Option<impl ToTokens + 's> {
    match (
        opts.async_strategy(),
        generics::has_any_async(trait_fns.map(|trait_fn| trait_fn.sig())),
    ) {
        (SpanOpt(AsyncStrategy::AsyncTrait, span), true) => {
            Some(attributes::Attr(attributes::AsyncTraitParams {
                crate_idents,
                span,
            }))
        }
        _ => None,
    }
}
