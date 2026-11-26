//! Where itidy knowingly departs from HTML Tidy 5.8.0.
//!
//! tidy 5.8.0's tables stop at the HTML5 of 2014. A page written against
//! the HTML Living Standard uses attributes those tables do not hold, and
//! tidy reports each as proprietary; it also escapes, and warns about,
//! every non-ASCII character of a URL, which HTML5 allows as written.
//! Both reports are wrong about a valid page, so itidy does not make them.
//! The tables extracted from tidy stay as extracted; what is added to them
//! is listed here and nowhere else.

/// Attributes every element takes in the HTML Living Standard and in
/// WAI-ARIA 1.2 that tidy 5.8.0's tables lack.
const GLOBAL: &[&str] = &[
    "aria-braillelabel",
    "aria-brailleroledescription",
    "aria-colcount",
    "aria-colindex",
    "aria-colindextext",
    "aria-colspan",
    "aria-current",
    "aria-description",
    "aria-details",
    "aria-errormessage",
    "aria-keyshortcuts",
    "aria-modal",
    "aria-placeholder",
    "aria-roledescription",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowindextext",
    "aria-rowspan",
    "autocapitalize",
    "autocorrect",
    "enterkeyhint",
    "exportparts",
    "inert",
    "inputmode",
    "is",
    "itemid",
    "itemprop",
    "itemref",
    "itemscope",
    "itemtype",
    "nonce",
    "part",
    "popover",
    "slot",
    "writingsuggestions",
];

/// Attributes of single elements in the HTML Living Standard that tidy
/// 5.8.0's tables lack.
const BY_ELEMENT: &[(&str, &[&str])] = &[
    ("a", &["ping", "referrerpolicy"]),
    ("area", &["ping", "referrerpolicy"]),
    ("button", &["popovertarget", "popovertargetaction"]),
    ("details", &["name"]),
    (
        "iframe",
        &[
            "allow",
            "allowfullscreen",
            "loading",
            "referrerpolicy",
            "srcdoc",
        ],
    ),
    (
        "img",
        &[
            "decoding",
            "fetchpriority",
            "loading",
            "referrerpolicy",
            "sizes",
            "srcset",
        ],
    ),
    ("input", &["popovertarget", "popovertargetaction"]),
    (
        "link",
        &[
            "as",
            "blocking",
            "color",
            "crossorigin",
            "disabled",
            "fetchpriority",
            "imagesizes",
            "imagesrcset",
            "integrity",
            "referrerpolicy",
        ],
    ),
    ("meta", &["media"]),
    (
        "script",
        &[
            "blocking",
            "crossorigin",
            "fetchpriority",
            "integrity",
            "nomodule",
            "referrerpolicy",
        ],
    ),
    ("source", &["height", "media", "sizes", "srcset", "width"]),
    ("style", &["blocking"]),
    (
        "template",
        &[
            "shadowrootclonable",
            "shadowrootdelegatesfocus",
            "shadowrootmode",
            "shadowrootserializable",
        ],
    ),
    ("video", &["playsinline"]),
];

/// Whether `attribute` on a `tag` element is an attribute of the HTML Living
/// Standard that tidy 5.8.0 does not know. Both names are lower case.
pub fn is_living_standard_attribute(tag: &str, attribute: &str) -> bool {
    GLOBAL.contains(&attribute)
        || BY_ELEMENT
            .iter()
            .any(|(element, attributes)| *element == tag && attributes.contains(&attribute))
}
