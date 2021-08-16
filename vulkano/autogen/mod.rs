// Copyright (c) 2021 The Vulkano developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

use indexmap::IndexMap;
use quote::quote;
use std::{collections::HashMap, io::Write, path::Path};
use vk_parse::{
    Extension, ExtensionChild, Feature, InterfaceItem, Registry, RegistryChild, Type,
    TypeCodeMarkup, TypeSpec, TypesChild,
};

mod extensions;
mod features;
mod fns;
mod properties;

pub fn write<W: Write>(writer: &mut W) {
    let registry = get_registry("vk.xml");
    let aliases = get_aliases(&registry);
    let extensions = get_extensions(&registry);
    let features = get_features(&registry);
    let types = get_types(&registry, &aliases, &features, &extensions);
    let header_version = get_header_version(&registry);

    let out_extensions = extensions::write(&extensions);
    let out_features = features::write(&types, &extensions);
    let out_fns = fns::write(&extensions);
    let out_properties = properties::write(&types, &extensions);

    write!(
        writer,
        "\
        // This file is auto-generated by vulkano autogen from vk.xml header version {}.\n\
        // It should not be edited manually. Changes should be made by editing autogen.\n\
        \n\
        {}",
        header_version,
        quote! {
            #out_extensions
            #out_features
            #out_fns
            #out_properties
        }
    )
    .unwrap();
}

fn get_registry<P: AsRef<Path> + ?Sized>(path: &P) -> Registry {
    let (registry, errors) = vk_parse::parse_file(path.as_ref()).unwrap();

    if !errors.is_empty() {
        eprintln!("The following errors were found while parsing the file:");

        for error in errors {
            eprintln!("{:?}", error);
        }
    }

    registry
}

fn get_aliases(registry: &Registry) -> HashMap<&str, &str> {
    registry
        .0
        .iter()
        .filter_map(|child| {
            if let RegistryChild::Types(types) = child {
                return Some(types.children.iter().filter_map(|ty| {
                    if let TypesChild::Type(ty) = ty {
                        if let Some(alias) = ty.alias.as_ref().map(|s| s.as_str()) {
                            return Some((ty.name.as_ref().unwrap().as_str(), alias));
                        }
                    }
                    None
                }));
            }
            None
        })
        .flatten()
        .collect()
}

fn get_extensions(registry: &Registry) -> IndexMap<&str, &Extension> {
    let iter = registry
        .0
        .iter()
        .filter_map(|child| {
            if let RegistryChild::Extensions(ext) = child {
                return Some(ext.children.iter().filter_map(|ext| {
                    if ext.supported.as_ref().map(|s| s.as_str()) == Some("vulkan")
                        && ext.obsoletedby.is_none()
                    {
                        return Some(ext);
                    }
                    None
                }));
            }
            None
        })
        .flatten();

    let extensions: HashMap<&str, &Extension> =
        iter.clone().map(|ext| (ext.name.as_str(), ext)).collect();
    let mut names: Vec<_> = iter.map(|ext| ext.name.as_str()).collect();
    names.sort_unstable_by_key(|name| {
        if name.starts_with("VK_KHR_") {
            (0, name.to_owned())
        } else if name.starts_with("VK_EXT_") {
            (1, name.to_owned())
        } else {
            (2, name.to_owned())
        }
    });

    names.iter().map(|&name| (name, extensions[name])).collect()
}

fn get_features(registry: &Registry) -> IndexMap<&str, &Feature> {
    registry
        .0
        .iter()
        .filter_map(|child| {
            if let RegistryChild::Feature(feat) = child {
                return Some((feat.name.as_str(), feat));
            }

            None
        })
        .collect()
}

fn get_types<'a>(
    registry: &'a Registry,
    aliases: &'a HashMap<&str, &str>,
    features: &'a IndexMap<&str, &Feature>,
    extensions: &'a IndexMap<&str, &Extension>,
) -> HashMap<&'a str, (&'a Type, Vec<&'a str>)> {
    let mut types: HashMap<&str, (&Type, Vec<&str>)> = registry
        .0
        .iter()
        .filter_map(|child| {
            if let RegistryChild::Types(types) = child {
                return Some(types.children.iter().filter_map(|ty| {
                    if let TypesChild::Type(ty) = ty {
                        if ty.alias.is_none() {
                            return ty.name.as_ref().map(|name| (name.as_str(), (ty, vec![])));
                        }
                    }
                    None
                }));
            }
            None
        })
        .flatten()
        .collect();

    features
        .iter()
        .map(|(name, feature)| (name, &feature.children))
        .chain(extensions.iter().map(|(name, ext)| (name, &ext.children)))
        .for_each(|(provided_by, children)| {
            children
                .iter()
                .filter_map(|child| {
                    if let ExtensionChild::Require { items, .. } = child {
                        return Some(items.iter());
                    }
                    None
                })
                .flatten()
                .filter_map(|item| {
                    if let InterfaceItem::Type { name, .. } = item {
                        return Some(name.as_str());
                    }
                    None
                })
                .for_each(|item_name| {
                    let item_name = aliases.get(item_name).unwrap_or(&item_name);
                    if let Some(ty) = types.get_mut(item_name) {
                        if !ty.1.contains(provided_by) {
                            ty.1.push(provided_by);
                        }
                    }
                });
        });

    types
        .into_iter()
        .filter(|(_key, val)| !val.1.is_empty())
        .collect()
}

fn get_header_version(registry: &Registry) -> u16 {
    registry.0.iter()
        .find_map(|child| -> Option<u16> {
            if let RegistryChild::Types(types) = child {
                return types.children.iter().find_map(|ty| -> Option<u16> {
                    if let TypesChild::Type(ty) = ty {
                        if let TypeSpec::Code(code) = &ty.spec {
                            if code.markup.iter().any(|mkup| matches!(mkup, TypeCodeMarkup::Name(name) if name == "VK_HEADER_VERSION")) {
                                return Some(code.code.rsplit_once(' ').unwrap().1.parse().unwrap());
                            }
                        }
                    }

                    None
                });
            }

            None
        })
        .unwrap()
}