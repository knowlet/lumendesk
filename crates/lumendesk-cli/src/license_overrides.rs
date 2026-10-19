//! Reviewed notice supplements for exact, immutable registry package identities.
//!
//! See ../third-party/overrides.json for source revisions and text digests.
//! A non-empty notice list is not a compliance certification. In particular,
//! objc2 upstream's Apple SDK caveat and absent MIT attribution remain visible.

#[derive(Clone, Copy)]
pub(crate) struct Notice {
    pub(crate) path: &'static str,
    pub(crate) bytes: &'static [u8],
    pub(crate) provenance: &'static str,
}

struct Override {
    name: &'static str,
    version: &'static str,
    license: &'static str,
    checksum: &'static str,
    notices: &'static [Notice],
    review: &'static [&'static str],
}

const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

static OVERRIDES: &[Override] = &[
    Override {
        name: "block2",
        version: "0.6.2",
        license: "MIT",
        checksum: "cdeb9d870516001442e364c5220d3574d2da8dc765554b4a617230d33fa58ef5",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/b4167b582b2f75f9a1be75495c41b765344fd03c/LICENSE.md",
            },
            Notice {
                path: "LICENSE-MIT-STANDARD.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-MIT-STANDARD.txt"),
                provenance: "https://raw.githubusercontent.com/spdx/license-list-data/279b7e997a87c09be00dd6bdda189519ade502cf/text/MIT.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
            "Upstream supplies an MIT identifier/link without a complete copyright-specific notice. The included unmodified MIT standard text retains its placeholders; it does not invent an attribution or establish that the notice obligation is complete.",
        ],
    },
    Override {
        name: "cesu8",
        version: "1.1.0",
        license: "Apache-2.0/MIT",
        checksum: "6d43a04d8753f35258c91f8ec639f792891f748a1edbd759cf1dcea3382ad83c",
        notices: &[
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
            Notice {
                path: "NOTICE-ATTRIBUTION.txt",
                bytes: include_bytes!("../third-party/notices/NOTICE-CESU8-ATTRIBUTION.txt"),
                provenance: "https://static.crates.io/crates/cesu8/cesu8-1.1.0.crate",
            },
        ],
        review: &[],
    },
    Override {
        name: "dispatch2",
        version: "0.3.1",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "1e0e367e4e7da84520dedcac1901e4da967309406d1e51017ae1abfb97adbd38",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/8852b424193ca41602281b3d7540d7c8ed51e49a/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "dlopen2",
        version: "0.8.2",
        license: "MIT",
        checksum: "5e2c5bd4158e66d1e215c49b837e11d62f3267b30c92f1d171c4d3105e3dc4d4",
        notices: &[Notice {
            path: "LICENSE-MIT.txt",
            bytes: include_bytes!("../third-party/notices/LICENSE-DLOPEN2-MIT.txt"),
            provenance: "https://raw.githubusercontent.com/OpenByteDev/dlopen2/cc80e4a0a90d499b677fdf7743699b4b3a43a989/LICENSE",
        }],
        review: &[],
    },
    Override {
        name: "dlopen2_derive",
        version: "0.4.3",
        license: "MIT",
        checksum: "0fbbb781877580993a8707ec48672673ec7b81eeba04cfd2310bd28c08e47c8f",
        notices: &[Notice {
            path: "LICENSE-MIT.txt",
            bytes: include_bytes!("../third-party/notices/LICENSE-DLOPEN2-MIT.txt"),
            provenance: "https://raw.githubusercontent.com/OpenByteDev/dlopen2/cc80e4a0a90d499b677fdf7743699b4b3a43a989/LICENSE",
        }],
        review: &[],
    },
    Override {
        name: "jni-sys-macros",
        version: "0.4.1",
        license: "MIT OR Apache-2.0",
        checksum: "38c0b942f458fe50cdac086d2f946512305e5631e720728f2a61aabcd47a6264",
        notices: &[
            Notice {
                path: "LICENSE-MIT.txt",
                bytes: include_bytes!("../third-party/notices/JNI-SYS-LICENSE-MIT.txt"),
                provenance: "https://raw.githubusercontent.com/jni-rs/jni-sys/64d77b7a5f119d7b55b4e2c169a4668067ff59e6/LICENSE-MIT",
            },
            Notice {
                path: "LICENSE-APACHE.txt",
                bytes: include_bytes!("../third-party/notices/JNI-SYS-LICENSE-APACHE.txt"),
                provenance: "https://raw.githubusercontent.com/jni-rs/jni-sys/64d77b7a5f119d7b55b4e2c169a4668067ff59e6/LICENSE-APACHE",
            },
        ],
        review: &[],
    },
    Override {
        name: "ndk",
        version: "0.9.0",
        license: "MIT OR Apache-2.0",
        checksum: "c3f42e7bbe13d351b6bead8286a43aac9534b82bd3cc43e47037f012ebfd62d4",
        notices: &[Notice {
            path: "LICENSE-APACHE.txt",
            bytes: include_bytes!("../third-party/notices/NDK-LICENSE-APACHE.txt"),
            provenance: "https://raw.githubusercontent.com/rust-mobile/ndk/49bbbba16c58ff63cb8a0ad0eca5a9fb7ecaec25/LICENSE-APACHE",
        }],
        review: &[],
    },
    Override {
        name: "ndk-sys",
        version: "0.6.0+11769913",
        license: "MIT OR Apache-2.0",
        checksum: "ee6cda3051665f1fb8d9e08fc35c96d5a244fb1be711a03b71118828afc9a873",
        notices: &[Notice {
            path: "LICENSE-APACHE.txt",
            bytes: include_bytes!("../third-party/notices/NDK-LICENSE-APACHE.txt"),
            provenance: "https://raw.githubusercontent.com/rust-mobile/ndk/49bbbba16c58ff63cb8a0ad0eca5a9fb7ecaec25/LICENSE-APACHE",
        }],
        review: &[],
    },
    Override {
        name: "objc2",
        version: "0.6.5",
        license: "MIT",
        checksum: "08849bbd4767dfae9457696856ae1c84fe4e0281bbe4a7abff2d0e06fb7981f8",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/d7d2fa23ceaa5e6096c923b081040e5d81b3b9df/LICENSE.md",
            },
            Notice {
                path: "LICENSE-MIT-STANDARD.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-MIT-STANDARD.txt"),
                provenance: "https://raw.githubusercontent.com/spdx/license-list-data/279b7e997a87c09be00dd6bdda189519ade502cf/text/MIT.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
            "Upstream supplies an MIT identifier/link without a complete copyright-specific notice. The included unmodified MIT standard text retains its placeholders; it does not invent an attribution or establish that the notice obligation is complete.",
        ],
    },
    Override {
        name: "objc2-app-kit",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "d49e936b501e5c5bf01fda3a9452ff86dc3ea98ad5f283e1455153142d97518c",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-cloud-kit",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "73ad74d880bb43877038da939b7427bba67e9dd42004a18b809ba7d87cee241c",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-core-data",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "0b402a653efbb5e82ce4df10683b6b28027616a2715e90009947d50b8dd298fa",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-core-foundation",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "2a180dd8642fa45cdb7dd721cd4c11b1cadd4929ce112ebd8b9f5803cc79d536",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-core-graphics",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "e022c9d066895efa1345f8e33e584b9f958da2fd4cd116792e15e07e4720a807",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-core-image",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "e5d563b38d2b97209f8e861173de434bd0214cf020e3423a52624cd1d989f006",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-core-location",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "ca347214e24bc973fc025fd0d36ebb179ff30536ed1f80252706db19ee452009",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-core-text",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "0cde0dfb48d25d2b4862161a4d5fcc0e3c24367869ad306b0c9ec0073bfed92d",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-encode",
        version: "4.1.0",
        license: "MIT",
        checksum: "ef25abbcd74fb2609453eb695bd2f860d389e457f67dc17cafc8b8cbc89d0c33",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/8d214f5477365ffcbcbb7de058c86ed9a518efb7/LICENSE.md",
            },
            Notice {
                path: "LICENSE-MIT-STANDARD.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-MIT-STANDARD.txt"),
                provenance: "https://raw.githubusercontent.com/spdx/license-list-data/279b7e997a87c09be00dd6bdda189519ade502cf/text/MIT.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
            "Upstream supplies an MIT identifier/link without a complete copyright-specific notice. The included unmodified MIT standard text retains its placeholders; it does not invent an attribution or establish that the notice obligation is complete.",
        ],
    },
    Override {
        name: "objc2-exception-helper",
        version: "0.1.1",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "c7a1c5fbb72d7735b076bb47b578523aedc40f3c439bea6dfd595c089d79d98a",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/8d214f5477365ffcbcbb7de058c86ed9a518efb7/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-foundation",
        version: "0.3.2",
        license: "MIT",
        checksum: "e3e0adef53c21f888deb4fa59fc59f7eb17404926ee8a6f59f5df0fd7f9f3272",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-MIT-STANDARD.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-MIT-STANDARD.txt"),
                provenance: "https://raw.githubusercontent.com/spdx/license-list-data/279b7e997a87c09be00dd6bdda189519ade502cf/text/MIT.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
            "Upstream supplies an MIT identifier/link without a complete copyright-specific notice. The included unmodified MIT standard text retains its placeholders; it does not invent an attribution or establish that the notice obligation is complete.",
        ],
    },
    Override {
        name: "objc2-io-surface",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "180788110936d59bab6bd83b6060ffdfffb3b922ba1396b312ae795e1de9d81d",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-quartz-core",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "96c1358452b371bf9f104e21ec536d37a650eb10f7ee379fff67d2e08d537f1f",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-ui-kit",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "d87d638e33c06f577498cbcc50491496a3ed4246998a7fbba7ccb98b1e7eab22",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-user-notifications",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "9df9128cbbfef73cda168416ccf7f837b62737d748333bfe9ab71c245d76613e",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "objc2-web-kit",
        version: "0.3.2",
        license: "Zlib OR Apache-2.0 OR MIT",
        checksum: "b2e5aaab980c433cf470df9d7af96a7b46a9d892d521a2cbbb2f8a4c16751e7f",
        notices: &[
            Notice {
                path: "LICENSE-UPSTREAM.md",
                bytes: include_bytes!("../third-party/notices/LICENSE-OBJC2-UPSTREAM.md"),
                provenance: "https://raw.githubusercontent.com/madsmtm/objc2/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md",
            },
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
        ],
        review: &[
            "The pinned upstream objc2 LICENSE.md identifies unresolved Apple SDK-derived binding redistribution implications. Review the applicable SDK terms for material in this target build.",
        ],
    },
    Override {
        name: "selectors",
        version: "0.38.0",
        license: "MPL-2.0",
        checksum: "8adfa1c298912827b8a28b223b3b874357397ae706e6190acd9bf28cee99114d",
        notices: &[Notice {
            path: "LICENSE-MPL-2.0.txt",
            bytes: include_bytes!("../third-party/notices/LICENSE-MPL-2.0.txt"),
            provenance: "https://www.mozilla.org/MPL/2.0/index.txt",
        }],
        review: &[],
    },
    Override {
        name: "webview2-com",
        version: "0.39.1",
        license: "MIT",
        checksum: "3f89fca7a704cee10dcb3654c1dbb8941d1783132f1917358af75bec37a7d7e6",
        notices: &[Notice {
            path: "LICENSE-RUST-MIT.txt",
            bytes: include_bytes!("../third-party/notices/LICENSE-WEBVIEW2-RUST-MIT.txt"),
            provenance: "https://raw.githubusercontent.com/wravery/webview2-rs/edc2caf886175ccaebe86078c9cfe1ae2a187328/LICENSE",
        }],
        review: &[],
    },
    Override {
        name: "webview2-com-macros",
        version: "0.8.1",
        license: "MIT",
        checksum: "67a921c1b6914c367b2b823cd4cde6f96beec77d30a939c8199bb377cf9b9b54",
        notices: &[Notice {
            path: "LICENSE-RUST-MIT.txt",
            bytes: include_bytes!("../third-party/notices/LICENSE-WEBVIEW2-RUST-MIT.txt"),
            provenance: "https://raw.githubusercontent.com/wravery/webview2-rs/dffa41a8a46d3f5565eefbff2de57d38d399f158/LICENSE",
        }],
        review: &[],
    },
    Override {
        name: "webview2-com-sys",
        version: "0.39.1",
        license: "MIT",
        checksum: "b3a07132775117d6065853d9d1178157b8c90e228de47129d6bce2c7edebedfb",
        notices: &[
            Notice {
                path: "LICENSE-RUST-MIT.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-WEBVIEW2-RUST-MIT.txt"),
                provenance: "https://raw.githubusercontent.com/wravery/webview2-rs/edc2caf886175ccaebe86078c9cfe1ae2a187328/LICENSE",
            },
            Notice {
                path: "MICROSOFT-SDK-LICENSE.txt",
                bytes: include_bytes!("../third-party/notices/MICROSOFT-WEBVIEW2-SDK-LICENSE.txt"),
                provenance: "https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/1.0.3800.47/microsoft.web.webview2.1.0.3800.47.nupkg#LICENSE.txt",
            },
            Notice {
                path: "MICROSOFT-SDK-NOTICE.txt",
                bytes: include_bytes!("../third-party/notices/MICROSOFT-WEBVIEW2-SDK-NOTICE.txt"),
                provenance: "https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/1.0.3800.47/microsoft.web.webview2.1.0.3800.47.nupkg#NOTICE.txt",
            },
        ],
        review: &[],
    },
    Override {
        name: "winapi-i686-pc-windows-gnu",
        version: "0.4.0",
        license: "MIT/Apache-2.0",
        checksum: "ac3b87c63620426dd9b991e5ce0329eff545bccbbb34f3be09ff6fb6ab51b7b6",
        notices: &[
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
            Notice {
                path: "NOTICE-ATTRIBUTION.txt",
                bytes: include_bytes!(
                    "../third-party/notices/NOTICE-WINAPI-IMPORT-ATTRIBUTION.txt"
                ),
                provenance: "https://static.crates.io/crates/winapi-i686-pc-windows-gnu/winapi-i686-pc-windows-gnu-0.4.0.crate",
            },
        ],
        review: &[],
    },
    Override {
        name: "winapi-x86_64-pc-windows-gnu",
        version: "0.4.0",
        license: "MIT/Apache-2.0",
        checksum: "712e227841d057c1ee1cd2fb22fa7e5a5461ae8e48fa2ca79ec42cfc1931183f",
        notices: &[
            Notice {
                path: "LICENSE-APACHE-2.0.txt",
                bytes: include_bytes!("../third-party/notices/LICENSE-APACHE-2.0.txt"),
                provenance: "https://www.apache.org/licenses/LICENSE-2.0.txt",
            },
            Notice {
                path: "NOTICE-ATTRIBUTION.txt",
                bytes: include_bytes!(
                    "../third-party/notices/NOTICE-WINAPI-IMPORT-ATTRIBUTION.txt"
                ),
                provenance: "https://static.crates.io/crates/winapi-x86_64-pc-windows-gnu/winapi-x86_64-pc-windows-gnu-0.4.0.crate",
            },
        ],
        review: &[],
    },
    Override {
        name: "epaint_default_fonts",
        version: "0.36.2",
        license: "(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0",
        checksum: "773fa9c96dd0dbef887e39d0ed6177f141cce4d68e6041df77570aa3702dfa13",
        notices: &[
            Notice {
                path: "LICENSE-MIT.txt",
                bytes: include_bytes!("../third-party/notices/EGUI-LICENSE-MIT.txt"),
                provenance: "https://raw.githubusercontent.com/emilk/egui/49682f8baa058bf49e011035cfbd6e825f88a5ef/LICENSE-MIT",
            },
            Notice {
                path: "LICENSE-APACHE.txt",
                bytes: include_bytes!("../third-party/notices/EGUI-LICENSE-APACHE.txt"),
                provenance: "https://raw.githubusercontent.com/emilk/egui/49682f8baa058bf49e011035cfbd6e825f88a5ef/LICENSE-APACHE",
            },
            Notice {
                path: "fonts/OFL.txt",
                bytes: include_bytes!("../third-party/notices/EPAINT-OFL.txt"),
                provenance: "https://static.crates.io/crates/epaint_default_fonts/epaint_default_fonts-0.36.2.crate#fonts/OFL.txt",
            },
            Notice {
                path: "fonts/UFL.txt",
                bytes: include_bytes!("../third-party/notices/EPAINT-UFL.txt"),
                provenance: "https://static.crates.io/crates/epaint_default_fonts/epaint_default_fonts-0.36.2.crate#fonts/UFL.txt",
            },
            Notice {
                path: "fonts/emoji-icon-font-mit-license.txt",
                bytes: include_bytes!(
                    "../third-party/notices/EPAINT-emoji-icon-font-mit-license.txt"
                ),
                provenance: "https://static.crates.io/crates/epaint_default_fonts/epaint_default_fonts-0.36.2.crate#fonts/emoji-icon-font-mit-license.txt",
            },
            Notice {
                path: "fonts/Hack-Regular.txt",
                bytes: include_bytes!("../third-party/notices/EPAINT-Hack-Regular.txt"),
                provenance: "https://static.crates.io/crates/epaint_default_fonts/epaint_default_fonts-0.36.2.crate#fonts/Hack-Regular.txt",
            },
            Notice {
                path: "fonts/NOTICE-FONT-ATTRIBUTION.txt",
                bytes: include_bytes!("../third-party/notices/NOTICE-EPAINT-FONT-ATTRIBUTION.txt"),
                provenance: "https://static.crates.io/crates/epaint_default_fonts/epaint_default_fonts-0.36.2.crate",
            },
        ],
        review: &[],
    },
];

fn lookup(
    name: &str,
    version: &str,
    source: &str,
    license: Option<&str>,
    checksum: &str,
) -> Option<&'static Override> {
    if source != REGISTRY {
        return None;
    }
    OVERRIDES.iter().find(|entry| {
        entry.name == name
            && entry.version == version
            && Some(entry.license) == license
            && entry.checksum == checksum
    })
}

pub(crate) fn notices(
    name: &str,
    version: &str,
    source: &str,
    license: Option<&str>,
    checksum: &str,
) -> Vec<Notice> {
    lookup(name, version, source, license, checksum)
        .map(|entry| entry.notices.to_vec())
        .unwrap_or_default()
}

pub(crate) fn review_notes(
    name: &str,
    version: &str,
    source: &str,
    license: Option<&str>,
    checksum: &str,
) -> Vec<&'static str> {
    lookup(name, version, source, license, checksum)
        .map(|entry| entry.review.to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn supplements_do_not_cross_package_or_registry_identities() {
        let entry = &OVERRIDES[0];
        assert!(
            !notices(
                entry.name,
                entry.version,
                REGISTRY,
                Some(entry.license),
                entry.checksum
            )
            .is_empty()
        );
        for (name, version, source, license, checksum) in [
            (
                "different-package",
                entry.version,
                REGISTRY,
                Some(entry.license),
                entry.checksum,
            ),
            (
                entry.name,
                "999.0.0",
                REGISTRY,
                Some(entry.license),
                entry.checksum,
            ),
            (
                entry.name,
                entry.version,
                "registry+https://example.invalid/index",
                Some(entry.license),
                entry.checksum,
            ),
            (
                entry.name,
                entry.version,
                REGISTRY,
                Some("unreviewed-terms"),
                entry.checksum,
            ),
            (entry.name, entry.version, REGISTRY, None, entry.checksum),
            (
                entry.name,
                entry.version,
                REGISTRY,
                Some(entry.license),
                "different-checksum",
            ),
        ] {
            assert!(notices(name, version, source, license, checksum).is_empty());
            assert!(review_notes(name, version, source, license, checksum).is_empty());
        }
    }

    #[test]
    fn embedded_notice_bytes_match_reviewed_source_digests() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../third-party/overrides.json")).unwrap();
        let entries = manifest["entries"].as_array().unwrap();
        assert_eq!(entries.len(), OVERRIDES.len());
        for e in entries {
            let ns = notices(
                e["name"].as_str().unwrap(),
                e["version"].as_str().unwrap(),
                e["source"].as_str().unwrap(),
                e["declared_license"].as_str(),
                e["archive_sha256"].as_str().unwrap(),
            );
            let expected = e["notice_files"].as_array().unwrap();
            assert_eq!(ns.len(), expected.len());
            for (actual, expected) in ns.iter().zip(expected) {
                assert_eq!(actual.path, expected["archive_path"].as_str().unwrap());
                assert_eq!(actual.provenance, expected["url"].as_str().unwrap());
                assert_eq!(
                    format!("{:x}", Sha256::digest(actual.bytes)),
                    expected["sha256"].as_str().unwrap()
                );
            }
        }
    }

    #[test]
    fn copied_notice_does_not_hide_upstream_redistribution_caveats() {
        let entry = OVERRIDES.iter().find(|e| e.name == "objc2").unwrap();
        let notes = review_notes(
            entry.name,
            entry.version,
            REGISTRY,
            Some(entry.license),
            entry.checksum,
        );
        assert!(notes.iter().any(|note| note.contains("Apple SDK")));
        assert!(
            notes
                .iter()
                .any(|note| note.contains("does not invent an attribution"))
        );
    }
}
