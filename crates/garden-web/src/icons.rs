//! Line icons drawn on a 24×24 grid in the same pen weight as the plan.

use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Select,
    Plot,
    Bed,
    RectBed,
    Lawn,
    Paving,
    Gravel,
    Water,
    Path,
    Fence,
    Wall,
    Hedge,
    Structure,
    Plant,
    Undo,
    Redo,
    Duplicate,
    Delete,
    Fit,
    Leaf,
    Grid,
    Magnet,
    Close,
    Photo,
    #[allow(dead_code)] // used by the print card (task 7)
    Printer,
}

fn body(icon: Icon) -> AnyView {
    match icon {
        Icon::Select => view! { <path d="M6 3.5l11 8.2-5 1 2.8 5.6-2.2 1.1-2.8-5.6-3.8 3.4z" /> }.into_any(),
        Icon::Plot => view! { <path d="M4 5h16v14H4z" stroke-dasharray="4 2 1 2" /> }.into_any(),
        Icon::Bed => view! {
            <path d="M5 8c2-4 9-5 13-2s2 9-2 11-11 2-12-2 0-4 1-7z" />
            <path d="M9 10l2 1M13 13l2-1M10 15l1.5 .5" />
        }
        .into_any(),
        Icon::RectBed => view! {
            <path d="M4 6h16v12H4z" />
            <path d="M8 10l2 1M13 9l2 1M9 14l2-1M15 14l1.5 .5" />
        }
        .into_any(),
        Icon::Lawn => view! {
            <ellipse cx="12" cy="12" rx="9" ry="6.5" />
            <path d="M9 13l1-3M12 14l.5-3.5M15 13l-.5-3" />
        }
        .into_any(),
        Icon::Paving => view! { <path d="M4 4h16v16H4zM4 12h16M12 4v8M8 12v8M16 12v8" /> }.into_any(),
        Icon::Gravel => view! {
            <path d="M5 7l6-3 8 3 1 8-7 5-8-3z" />
            <circle cx="9" cy="10" r=".8" /><circle cx="14" cy="9" r=".8" />
            <circle cx="12" cy="14" r=".8" /><circle cx="16" cy="13" r=".8" /><circle cx="8" cy="14.5" r=".8" />
        }
        .into_any(),
        Icon::Water => view! {
            <ellipse cx="12" cy="12" rx="9" ry="6.5" />
            <path d="M7.5 11.5q2-1.5 4.5 0t4.5 0M9 14.5q1.5-1 3 0t3 0" />
        }
        .into_any(),
        Icon::Path => view! { <path d="M6 21c0-6 3-8 6-10s5-5 5-8M10 21c0-4 2-6 5-8s5-6 5-10" /> }.into_any(),
        Icon::Fence => view! { <path d="M3 13h18M3 9h18M6 5v14M12 5v14M18 5v14" /> }.into_any(),
        Icon::Wall => view! { <path d="M3 8h18v8H3zM9 8v4M15 8v4M6 12v4M12 12v4M18 12v4M3 12h18" /> }.into_any(),
        Icon::Hedge => view! {
            <path d="M3 15c1-3 3-3 4 0 1-3 3-3 4 0 1-3 3-3 4 0 1-3 3-3 4 0 1-1 1.5-1 2 0M3 15v3h18v-3" />
        }
        .into_any(),
        Icon::Structure => view! { <path d="M4 11l8-6 8 6v9H4zM10 20v-5h4v5" /> }.into_any(),
        Icon::Plant => view! {
            <path d="M12 3.5q2.5 1 3 3.5 3-.5 4 2-1 2.5-3 3 1.5 2.5-.5 4.5-2.5.5-3.5-1-1 2.5-3.5 1.5-1.5-2.5 0-4.5-3-.5-3.5-3 1.5-2.5 4-2 .5-2.5 3-4z" />
            <path d="M12 12v8.5" />
        }
        .into_any(),
        Icon::Leaf => view! { <path d="M12 21V11M12 11c0-4 3-7 7-7 0 4-3 7-7 7zM12 13c0-3-2.5-5.5-6-5.5 0 3 2.5 5.5 6 5.5z" /> }.into_any(),
        Icon::Undo => view! { <path d="M9 14L4 9l5-5M4 9h11a5 5 0 010 10h-4" /> }.into_any(),
        Icon::Redo => view! { <path d="M15 14l5-5-5-5M20 9H9a5 5 0 000 10h4" /> }.into_any(),
        Icon::Duplicate => view! { <path d="M8 8h11v11H8zM5 16V5h11" /> }.into_any(),
        Icon::Delete => view! { <path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13M10.5 11v6M13.5 11v6" /> }.into_any(),
        Icon::Grid => view! {
            <circle cx="6" cy="6" r="1.2" /><circle cx="12" cy="6" r="1.2" /><circle cx="18" cy="6" r="1.2" />
            <circle cx="6" cy="12" r="1.2" /><circle cx="12" cy="12" r="1.2" /><circle cx="18" cy="12" r="1.2" />
            <circle cx="6" cy="18" r="1.2" /><circle cx="12" cy="18" r="1.2" /><circle cx="18" cy="18" r="1.2" />
        }
        .into_any(),
        Icon::Magnet => view! { <path d="M6 4v8a6 6 0 0012 0V4h-4v8a2 2 0 01-4 0V4zM6 8h4M14 8h4" /> }.into_any(),
        Icon::Close => view! { <path d="M6 6l12 12M18 6L6 18" /> }.into_any(),
        Icon::Photo => view! { <path d="M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4" /><circle cx="15.5" cy="9" r="1.5" /> }.into_any(),
        Icon::Printer => view! { <path d="M7 9V4h10v5M7 17H4v-7h16v7h-3M7 14h10v6H7z" /> }.into_any(),
        Icon::Fit => view! { <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5M9 9h6v6H9z" /> }.into_any(),
    }
}

pub fn icon(icon: Icon) -> impl IntoView {
    view! {
        <svg class="icon" viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor"
            stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            {body(icon)}
        </svg>
    }
}
