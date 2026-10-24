use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    *,
};

struct Entry {
    path: Tokens,
    layout: Tokens,
    item: ItemTrait,
}
impl Parse for Entry {
    fn parse(input: ParseStream) -> Result<Self> {
        let p;

        bracketed!(p in input);

        let l;

        bracketed!(l in input);

        Ok(Self {
            path: p.parse()?,
            layout: l.parse()?,
            item: input.parse()?,
        })
    }
}

struct Expansion {
    chain: Vec<Entry>,
    mode: Ident,
    rest: Tokens,
}

impl Parse for Expansion {
    fn parse(input: ParseStream) -> Result<Self> {
        let c;

        bracketed!(c in input);

        let mut chain = Vec::new();

        while !c.is_empty() {
            chain.push(c.parse()?);
        }

        Ok(Self {
            chain,
            mode: input.parse()?,
            rest: input.parse()?,
        })
    }
}

fn chain_tokens(chain: &[Entry]) -> Tokens {
    let entries = chain.iter().map(|e| {
        let Entry { path, layout, item } = e;
        quote!([#path][#layout] #item)
    });

    quote!(#(#entries)*)
}

fn error(e: Error) -> TokenStream {
    e.into_compile_error().into()
}

#[proc_macro_attribute]
pub fn vtable(attr: TokenStream, item: TokenStream) -> TokenStream {
    let t = parse_macro_input!(item as ItemTrait);
    let attr: Tokens = attr.into();

    let parents: Vec<_> = t
        .supertraits
        .iter()
        .filter_map(|b| match b {
            TypeParamBound::Trait(b) => Some(&b.path),
            _ => None,
        })
        .collect();

    if parents.len() > 1 {
        return error(Error::new(t.span(), "only one native parent is supported"));
    }

    if let Some(p) = parents.first() {
        quote!(#p! { define [#attr] #t }).into()
    } else {
        define(Vec::new(), attr, t)
            .unwrap_or_else(Error::into_compile_error)
            .into()
    }
}

#[proc_macro_attribute]
pub fn vtable_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return error(Error::new(
            proc_macro2::Span::call_site(),
            "vtable_impl takes no arguments",
        ));
    }

    let i = parse_macro_input!(item as ItemImpl);
    let Some((_, p, _)) = &i.trait_ else {
        return error(Error::new(i.span(), "expected a trait impl"));
    };

    quote!(#p! { implement #i }).into()
}

#[proc_macro]
pub fn __expand(input: TokenStream) -> TokenStream {
    let e = parse_macro_input!(input as Expansion);
    let result = if e.mode == "define" {
        struct Definition {
            attr: Tokens,
            item: ItemTrait,
        }

        impl Parse for Definition {
            fn parse(i: ParseStream) -> Result<Self> {
                let a;
                bracketed!(a in i);
                Ok(Self {
                    attr: a.parse()?,
                    item: i.parse()?,
                })
            }
        }
        syn::parse2::<Definition>(e.rest).and_then(|d| define(e.chain, d.attr, d.item))
    } else {
        syn::parse2::<ItemImpl>(e.rest).and_then(|i| implement(e.chain, i))
    };

    result.unwrap_or_else(Error::into_compile_error).into()
}

fn methods(t: &ItemTrait) -> impl Iterator<Item = &TraitItemFn> {
    t.items.iter().filter_map(|i| {
        if let TraitItem::Fn(m) = i {
            Some(m)
        } else {
            None
        }
    })
}

#[derive(Default)]
struct MethodOptions {
    offset: Option<usize>,
    raw_this: bool,
}

fn method_options(attrs: &[Attribute]) -> Result<MethodOptions> {
    let mut options = MethodOptions::default();

    for a in attrs {
        if a.path().is_ident("vtable") {
            a.parse_nested_meta(|m| {
                if m.path.is_ident("raw_this") {
                    options.raw_this = true;
                    Ok(())
                } else if m.path.is_ident("offset") {
                    let lit: LitInt = m.value()?.parse()?;
                    let offset = lit.base10_parse::<usize>()?;

                    if offset % 8 != 0 {
                        return Err(Error::new(
                            lit.span(),
                            "byte offsets must be multiples of eight",
                        ));
                    }

                    options.offset = Some(offset / 8);

                    Ok(())
                } else {
                    Err(m.error("expected raw_this or offset"))
                }
            })?;
        }
    }

    Ok(options)
}

fn contains_self(t: Tokens) -> bool {
    t.into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == "Self",
        proc_macro2::TokenTree::Group(g) => contains_self(g.stream()),
        _ => false,
    })
}

// Only direct pointers to Self are erased; arbitrary Self-associated ABI types are rejected.
fn abi_type(t: &Type) -> Result<(Tokens, Tokens, bool)> {
    if let Type::Ptr(p) = t
        && let Type::Path(s) = &*p.elem
        && s.path.is_ident("Self")
    {
        return Ok(if p.mutability.is_some() {
            (quote!(*mut ()), quote!(*mut T), true)
        } else {
            (quote!(*const ()), quote!(*const T), true)
        });
    }

    if contains_self(quote!(#t)) {
        return Err(Error::new(
            t.span(),
            "only direct raw pointers to Self are supported in ABI arguments/returns",
        ));
    }

    Ok((quote!(#t), quote!(#t), false))
}

fn define(chain: Vec<Entry>, attr: Tokens, mut t: ItemTrait) -> Result<Tokens> {
    if t.unsafety.is_none() || !t.generics.params.is_empty() {
        return Err(Error::new(
            t.span(),
            "vtable requires an unsafe, non-generic trait",
        ));
    }

    let name = t.ident.clone();
    let layout = format_ident!("{name}Layout");
    let vis = t.vis.clone();

    let canonical: Path = if attr.is_empty() {
        parse_quote!(#name)
    } else {
        let meta: MetaNameValue = syn::parse2(attr)?;
        if !meta.path.is_ident("path") {
            return Err(Error::new(meta.span(), "expected path = module::Trait"));
        }

        if let Expr::Path(p) = meta.value {
            p.path
        } else {
            return Err(Error::new(meta.span(), "expected a trait path"));
        }
    };

    if canonical.leading_colon.is_some()
        || canonical
            .segments
            .first()
            .is_some_and(|s| matches!(s.ident.to_string().as_str(), "crate" | "self" | "super"))
        || canonical.segments.last().is_none_or(|s| s.ident != name)
        || canonical
            .segments
            .iter()
            .any(|s| !matches!(s.arguments, PathArguments::None))
    {
        return Err(Error::new(
            canonical.span(),
            "path must be crate-root-relative and end with the trait name, e.g. ui::Screen",
        ));
    }

    let mut canonical_layout = canonical.clone();
    canonical_layout.segments.last_mut().unwrap().ident = layout.clone();

    let parent = chain.last().map(|e| e.layout.clone());
    let original = t.clone();
    let mut fields = Vec::new();
    let mut helpers = Vec::new();
    let mut defaults = Vec::new();
    let mut checks = Vec::new();
    let mut hidden_defaults = Vec::new();
    let mut slot_metadata = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut next_slot = 0usize;
    let mut fixed_width = false;

    for e in &chain {
        for m in methods(&e.item) {
            seen.insert(m.sig.ident.to_string());
            let options = method_options(&m.attrs)?;

            fixed_width |= options.offset.is_some();
            next_slot = options.offset.unwrap_or(next_slot) + 1;
        }
    }

    for item in &mut t.items {
        let TraitItem::Fn(m) = item else {
            return Err(Error::new(
                item.span(),
                "only virtual methods are supported",
            ));
        };

        let options = method_options(&m.attrs)?;
        m.attrs.retain(|a| !a.path().is_ident("vtable"));
        let sig = &m.sig;

        if sig.abi.is_some() || sig.generics.where_clause.is_some() {
            return Err(Error::new(
                sig.span(),
                "explicit method ABIs and where clauses are unsupported",
            ));
        }

        if !seen.insert(sig.ident.to_string()) {
            return Err(Error::new(
                sig.span(),
                "duplicate or shadowed virtual method",
            ));
        }

        if sig.unsafety.is_none()
            || sig.asyncness.is_some()
            || sig.constness.is_some()
            || !sig.generics.params.is_empty()
            || sig.variadic.is_some()
        {
            return Err(Error::new(
                sig.span(),
                "virtual methods must be unsafe, non-generic, non-async methods",
            ));
        }

        let (mutable, cast) = match sig.inputs.first() {
            Some(FnArg::Receiver(r)) if !options.raw_this => {
                if r.reference.is_none() || r.colon_token.is_some() {
                    return Err(Error::new(r.span(), "expected &self or &mut self receiver"));
                }

                let self_token = r.self_token;
                (
                    r.mutability.is_some(),
                    if r.mutability.is_some() {
                        quote!(#self_token as *mut Self)
                    } else {
                        quote!(#self_token as *const Self)
                    },
                )
            }

            Some(FnArg::Typed(a)) if options.raw_this => {
                let Type::Ptr(p) = &*a.ty else {
                    return Err(Error::new(
                        a.span(),
                        "raw_this requires this: *mut Self or *const Self",
                    ));
                };

                let (_, _, is_self) = abi_type(&a.ty)?;
                let Pat::Ident(n) = &*a.pat else {
                    return Err(Error::new(a.span(), "raw_this requires a named pointer"));
                };

                if !is_self {
                    return Err(Error::new(a.span(), "raw_this requires a pointer to Self"));
                }

                let n = &n.ident;
                (p.mutability.is_some(), quote!(#n))
            }
            _ => {
                return Err(Error::new(
                    sig.span(),
                    "expected &self, &mut self, or #[vtable(raw_this)]",
                ));
            }
        };

        let n = &sig.ident;
        let setter = format_ident!("__override_{}", n);
        let caller = format_ident!("call_{}", n);
        let offset = options.offset.unwrap_or(next_slot);
        fixed_width |= options.offset.is_some();

        if offset < next_slot {
            return Err(Error::new(
                sig.span(),
                "slot overlaps a previous method or parent prefix",
            ));
        }

        if offset > next_slot {
            let gap = format_ident!("__gap_{}", next_slot);
            let count = offset - next_slot;

            fields.push(quote!(pub #gap:[usize;#count]));
        }

        next_slot = offset + 1;
        slot_metadata.push(quote!(#offset=>Some(::native_vtable::Slot {name:concat!(stringify!(#name),"::",stringify!(#n)),index:#offset})));
        let offset_name = format_ident!("OFFSET_{}", n.to_string().to_uppercase());
        helpers.push(quote!(pub const #offset_name:usize=#offset*core::mem::size_of::<usize>();));

        let mut names = Vec::new();
        let mut types = Vec::new();
        let mut typed = Vec::new();
        let mut args = Vec::new();

        for a in sig.inputs.iter().skip(1) {
            let FnArg::Typed(a) = a else { unreachable!() };

            let Pat::Ident(p) = &*a.pat else {
                return Err(Error::new(a.span(), "arguments must have simple names"));
            };

            let (abi, concrete, is_self) = abi_type(&a.ty)?;
            let arg = &p.ident;

            names.push(arg.clone());
            types.push(abi);
            typed.push(concrete);

            args.push(if is_self {
                quote!(#arg.cast())
            } else {
                quote!(#arg)
            });
        }

        let (ret, typed_ret, cast_ret) = match &sig.output {
            ReturnType::Default => (quote!(), quote!(), false),
            ReturnType::Type(_, t) => {
                let (a, b, c) = abi_type(t)?;
                (quote!(->#a), quote!(->#b), c)
            }
        };

        let ret_cast = if cast_ret { quote!(.cast()) } else { quote!() };
        let ptr = if mutable {
            quote!(*mut ())
        } else {
            quote!(*const ())
        };

        let objptr = if mutable {
            quote!(*mut T)
        } else {
            quote!(*const T)
        };

        let borrow = if options.raw_this {
            quote!(this.cast::<T>())
        } else if mutable {
            quote!(&mut *this.cast::<T>())
        } else {
            quote!(&*this.cast::<T>())
        };

        fields.push(quote!(pub #n: Option<unsafe extern "C" fn(#ptr, #(#types),*) #ret>));
        checks.push(quote!(if self.#n.is_none() { return Some(concat!(stringify!(#name),"::",stringify!(#n))); }));
        helpers.push(quote! {
            #[doc(hidden)]
            pub fn #setter<T: #name>(&mut self) {
                unsafe extern "C" fn thunk<T: #name>(this:#ptr, #(#names:#types),*) #ret {
                    unsafe { <T as #name>::#n(#borrow, #(#args),*) #ret_cast }
                }
                self.#n=Some(thunk::<T>);
            }

            pub unsafe fn #caller<T>(&self, this:#objptr, #(#names:#typed),*) #typed_ret {
                unsafe { (self.#n.expect("unresolved virtual method"))(this.cast(), #(#args),*) #ret_cast }
            }
        });

        if m.default.is_some() {
            let default_name = format_ident!("__native_default_{}", n);
            let default_setter = format_ident!("__fallback_{}", n);
            let mut hidden = m.clone();

            hidden.sig.ident = default_name.clone();
            hidden.attrs = vec![parse_quote!(#[doc(hidden)])];
            hidden_defaults.push(TraitItem::Fn(hidden));

            helpers.push(quote! {
                #[doc(hidden)]
                pub fn #default_setter<T:#name>(&mut self) {
                    unsafe extern "C" fn thunk<T:#name>(this:#ptr, #(#names:#types),*) #ret {
                        unsafe { <T as #name>::#default_name(#borrow, #(#args),*) #ret_cast }
                    }

                    self.#n=Some(thunk::<T>);
                }
            });

            defaults.push(quote!(if self.#n.is_none() {self.#default_setter::<T>();}));
        }

        {
            m.default = Some(parse_quote!({
                unsafe {
                    let this=#cast;
                    let table=*(this.cast::<*const #layout>());
                    (*table).#caller(this, #(#names),*)
                }
            }));

            m.semi_token = None;
        }
    }

    t.items.extend(hidden_defaults);
    t.supertraits.push(parse_quote!(Sized));

    let (basefield, basecheck, basedefault) = if let Some(p) = &parent {
        (
            quote!(pub base:#p,),
            quote!(
                if let Some(n) = ::native_vtable::VTableLayout::missing(&self.base) {
                    return Some(n);
                }
            ),
            quote!(self.base.__defaults::<T>();),
        )
    } else {
        (quote!(), quote!(), quote!())
    };

    let prefixes = chain.iter().map(|e| {
        let p = &e.layout;
        quote!(unsafe impl ::native_vtable::PrefixOf<#layout> for #p {})
    });

    let previous = chain_tokens(&chain);
    let width_check = if fixed_width {
        quote!(
            const _: () = assert!(
                core::mem::size_of::<usize>() == 8,
                "explicit byte offsets require a 64-bit target"
            );
        )
    } else {
        quote!()
    };

    let parent_slot = if let Some(p) = &parent {
        quote!(<#p as ::native_vtable::VTableLayout>::slot(index))
    } else {
        quote!(None)
    };

    let defining_crate = quote_spanned!(name.span()=> $crate);
    let unique = format_ident!(
        "__native_vtable_{}",
        canonical
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("_")
    );

    let exported = quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #unique {
            ($($tail:tt)*) => {
                ::native_vtable::__expand! {
                    [#previous [#defining_crate::#canonical][#defining_crate::#canonical_layout] #original]
                    $($tail)*
                }
            };
        }
        #vis use #unique as #name;
    };

    Ok(quote! {
        #t
        #width_check
        #[repr(C)]
        #[derive(Clone, Copy)]
        #vis struct #layout { #basefield #(#fields,)* }
        unsafe impl ::native_vtable::VTableLayout for #layout {
            const SLOT_COUNT:usize=#next_slot;
            fn slot(index:usize)->Option<::native_vtable::Slot> {match index {#(#slot_metadata,)* _=>#parent_slot}}
            fn missing(&self)->Option<&'static str> {#basecheck #(#checks)* None}
        }

        unsafe impl ::native_vtable::PrefixOf<#layout> for #layout {}
        #(#prefixes)*
        impl #layout {
            #(#helpers)*
            #[doc(hidden)]
            pub fn __defaults<T:#name>(&mut self) {#basedefault #(#defaults)*}
        }

        #exported
    })
}

fn implement(chain: Vec<Entry>, i: ItemImpl) -> Result<Tokens> {
    if i.unsafety.is_none() || !i.generics.params.is_empty() {
        return Err(Error::new(
            i.span(),
            "expected unsafe impl for a concrete object type",
        ));
    }

    let ty = &i.self_ty;
    let mut supplied = std::collections::HashMap::new();

    for item in &i.items {
        let ImplItem::Fn(m) = item else {
            return Err(Error::new(
                item.span(),
                "only virtual methods are supported",
            ));
        };

        if supplied.insert(m.sig.ident.to_string(), m).is_some() {
            return Err(Error::new(m.span(), "duplicate override"));
        }
    }

    let mut impls = Vec::new();
    let mut patches = Vec::new();

    for (idx, e) in chain.iter().enumerate() {
        let p = &e.path;
        let mut body = Vec::new();

        for m in methods(&e.item) {
            if let Some(over) = supplied.remove(&m.sig.ident.to_string()) {
                body.push(over);
                let setter = format_ident!("__override_{}", m.sig.ident);
                let mut target = quote!(table);
                for _ in idx + 1..chain.len() {
                    target = quote!(#target.base);
                }

                patches.push(quote!(#target.#setter::<#ty>();));
            }
        }

        impls.push(quote!(unsafe impl #p for #ty {#(#body)*}));
    }

    if let Some(m) = supplied.values().next() {
        return Err(Error::new(
            m.span(),
            "method does not belong to this vtable hierarchy",
        ));
    }

    let layout = &chain
        .last()
        .ok_or_else(|| Error::new(i.span(), "missing trait metadata"))?
        .layout;

    Ok(quote! {
        #(#impls)*
        unsafe impl ::native_vtable::VTable for #ty {
            type Table=#layout;
            fn vtable_builder()->::native_vtable::Builder<Self::Table> {
                ::native_vtable::Builder::new(|table:&mut #layout| {
                    #(#patches)*
                    table.__defaults::<#ty>();
                })
            }
        }
    })
}

struct NativeArgs {
    resolve: Path,
}

impl Parse for NativeArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let key: Ident = input.parse()?;
        if key != "resolve" {
            return Err(Error::new(key.span(), "expected `resolve = path::to::resolver`"));
        }
        input.parse::<Token![=]>()?;
        let resolve = input.parse()?;
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the resolver path"));
        }
        Ok(Self { resolve })
    }
}

struct NativeDecl {
    attrs: Vec<Attribute>,
    vis: Visibility,
    sig: Signature,
}

impl Parse for NativeDecl {
    fn parse(input: ParseStream) -> Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis = input.parse()?;
        let sig = input.parse()?;
        input.parse::<Token![;]>()?;
        Ok(Self { attrs, vis, sig })
    }
}

fn take_symbol(attrs: &mut Vec<Attribute>, span: proc_macro2::Span) -> Result<LitStr> {
    let mut symbol = None;
    let mut rest = Vec::new();
    for a in attrs.drain(..) {
        if !a.path().is_ident("native") {
            rest.push(a);
            continue;
        }
        a.parse_nested_meta(|m| {
            if m.path.is_ident("symbol") {
                symbol = Some(m.value()?.parse::<LitStr>()?);
                Ok(())
            } else {
                Err(m.error("expected `symbol = \"...\"`"))
            }
        })?;
    }
    *attrs = rest;
    symbol.ok_or_else(|| Error::new(span, "missing #[native(symbol = \"...\")]"))
}

fn native_fn(decl: NativeDecl, resolve: &Path, in_impl: bool) -> Result<Tokens> {
    let NativeDecl { mut attrs, vis, mut sig } = decl;
    let symbol = take_symbol(&mut attrs, sig.ident.span())?;

    if sig.unsafety.is_none() {
        return Err(Error::new(sig.fn_token.span(), "native functions must be `unsafe fn`"));
    }
    if sig.asyncness.is_some() || sig.constness.is_some() || sig.variadic.is_some() {
        return Err(Error::new(sig.span(), "native functions cannot be async, const or variadic"));
    }
    if !sig.generics.params.is_empty() {
        return Err(Error::new(sig.generics.span(), "native functions cannot be generic"));
    }
    sig.abi = None;

    let mut ptr_args = Vec::new();
    let mut call_args = Vec::new();
    for (n, input) in sig.inputs.iter_mut().enumerate() {
        match input {
            FnArg::Receiver(r) => {
                if !in_impl {
                    return Err(Error::new(r.span(), "receivers are only allowed in impl blocks"));
                }
                if r.reference.is_none() {
                    return Err(Error::new(r.span(), "use `&self` or `&mut self` for native receivers"));
                }
                if r.mutability.is_some() {
                    ptr_args.push(quote!(*mut Self));
                    call_args.push(quote!(self as *mut Self));
                } else {
                    ptr_args.push(quote!(*const Self));
                    call_args.push(quote!(self as *const Self));
                }
            }
            FnArg::Typed(t) => {
                let name = format_ident!("__arg{}", n);
                *t.pat = parse_quote!(#name);
                let ty = &t.ty;
                ptr_args.push(quote!(#ty));
                call_args.push(quote!(#name));
            }
        }
    }

    let ret = match &sig.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, t) => quote!(#t),
    };
    sig.output = parse_quote!(-> ::core::result::Result<#ret, ::native_vtable::Unresolved>);

    Ok(quote! {
        #(#attrs)*
        #[allow(clippy::missing_safety_doc)]
        #vis #sig {
            static __ADDR: ::native_vtable::NativeAddr = ::native_vtable::NativeAddr::new();
            let __addr = __ADDR.get(#symbol, #resolve)?;
            let __f: unsafe extern "C" fn(#(#ptr_args),*) -> #ret =
                unsafe { ::core::mem::transmute::<usize, _>(__addr) };
            ::core::result::Result::Ok(unsafe { __f(#(#call_args),*) })
        }
    })
}

#[proc_macro_attribute]
pub fn native(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as NativeArgs);
    let item: Item = parse_macro_input!(item as Item);

    let result = match item {
        Item::Impl(mut i) => {
            if i.trait_.is_some() {
                return error(Error::new(i.span(), "#[native] needs an inherent impl block"));
            }
            let mut items = Vec::new();
            let mut err: Option<Error> = None;
            for it in std::mem::take(&mut i.items) {
                let tokens = match it {
                    ImplItem::Verbatim(v) => syn::parse2::<NativeDecl>(v)
                        .and_then(|d| native_fn(d, &args.resolve, true)),
                    other => Ok(quote!(#other)),
                };
                match tokens {
                    Ok(t) => items.push(t),
                    Err(e) => match &mut err {
                        Some(all) => all.combine(e),
                        None => err = Some(e),
                    },
                }
            }
            if let Some(e) = err {
                return error(e);
            }
            let ItemImpl { attrs, impl_token, generics, self_ty, .. } = &i;
            let (ig, _, wc) = generics.split_for_impl();
            Ok(quote!(#(#attrs)* #impl_token #ig #self_ty #wc { #(#items)* }))
        }
        Item::ForeignMod(m) => m
            .items
            .into_iter()
            .map(|it| match it {
                ForeignItem::Fn(f) => {
                    let mut sig = f.sig;
                    sig.unsafety = Some(Default::default());
                    native_fn(
                        NativeDecl { attrs: f.attrs, vis: f.vis, sig },
                        &args.resolve,
                        false,
                    )
                }
                other => Err(Error::new(other.span(), "only functions are supported")),
            })
            .collect::<Result<Vec<_>>>()
            .map(|fns| quote!(#(#fns)*)),
        other => Err(Error::new(
            other.span(),
            "#[native] goes on an inherent impl block or an extern block",
        )),
    };

    result.unwrap_or_else(Error::into_compile_error).into()
}
