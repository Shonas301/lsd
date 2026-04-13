use crate::color::{Colors, Elem};
use crate::flags::blocks::Block;
use crate::flags::{Display, Flags, HyperlinkOption, Layout};
use crate::git_theme::GitTheme;
use crate::icon::Icons;
use crate::meta::name::DisplayOption;
use crate::meta::{FileType, Meta, OwnerCache};
use std::collections::HashMap;
use term_grid::{Cell, Direction, Filling, Grid, GridOptions};
use terminal_size::terminal_size;
use unicode_width::UnicodeWidthStr;

const EDGE: &str = "\u{251c}\u{2500}\u{2500}"; // "├──"
const LINE: &str = "\u{2502}  "; // "│  "
const CORNER: &str = "\u{2514}\u{2500}\u{2500}"; // "└──"
const BLANK: &str = "   ";
const INDENT_STEP: usize = 2;
const TREE_COLUMN_GUTTER: usize = 2;

pub fn grid(
    metas: &[Meta],
    flags: &Flags,
    colors: &Colors,
    icons: &Icons,
    git_theme: &GitTheme,
) -> String {
    let term_width = terminal_size().map(|(w, _)| w.0 as usize);
    let owner_cache = OwnerCache::default();

    inner_display_grid(
        &DisplayOption::None,
        metas,
        &owner_cache,
        flags,
        colors,
        icons,
        git_theme,
        0,
        term_width,
    )
}

pub fn tree(
    metas: &[Meta],
    flags: &Flags,
    colors: &Colors,
    icons: &Icons,
    git_theme: &GitTheme,
) -> String {
    // resolve once at entry — Auto mode reads the top-level entry count and uses
    // that cap at every depth. see MaxShown::resolve.
    let top_count = compute_top_count(metas);
    let max_cap = flags.max_shown.resolve(top_count);

    if flags.tree_columns.0 {
        if !flags.max_shown.is_set() {
            eprintln!("lsd: --tree-columns requires --max-shown; falling back to vertical layout");
        } else if flags.blocks.0.len() > 1 {
            eprintln!(
                "lsd: --tree-columns only supports name-only output; falling back to vertical layout"
            );
        } else {
            // common case: user passed a single directory arg. in tree mode fetch()
            // produces one top-level meta wrapping the dir, so packing collapses to
            // one column. unwrap the root and pack its children instead, printing
            // the root name as a header line above the packed columns.
            if metas.len() == 1 {
                if let Some(children) = metas[0].content.as_deref() {
                    if !children.is_empty() {
                        let header =
                            render_tree_root_header(&metas[0], flags, colors, icons, git_theme);
                        let packed =
                            tree_columns(children, flags, colors, icons, git_theme, max_cap);
                        return if header.is_empty() {
                            packed
                        } else {
                            format!("{header}\n{packed}")
                        };
                    }
                }
            }
            return tree_columns(metas, flags, colors, icons, git_theme, max_cap);
        }
    }

    let mut grid = Grid::new(GridOptions {
        filling: Filling::Spaces(1),
        direction: Direction::LeftToRight,
    });

    let padding_rules = get_padding_rules(metas, flags);
    let mut index = 0;
    for (i, block) in flags.blocks.0.iter().enumerate() {
        if block == &Block::Name {
            index = i;
            break;
        }
    }

    let owner_cache = OwnerCache::default();

    for cell in inner_display_tree(
        metas,
        &owner_cache,
        flags,
        colors,
        icons,
        git_theme,
        (0, ""),
        &padding_rules,
        index,
        max_cap,
    ) {
        grid.add(cell);
    }

    grid.fit_into_columns(flags.blocks.0.len()).to_string()
}

// top-level entry count used to resolve MaxShown::Auto.
// single-arg directory: use its immediate children. otherwise: number of cli args.
fn compute_top_count(metas: &[Meta]) -> usize {
    if metas.len() == 1 {
        metas[0].content.as_deref().map_or(1, |c| c.len())
    } else {
        metas.len()
    }
}

fn tree_columns(
    metas: &[Meta],
    flags: &Flags,
    colors: &Colors,
    icons: &Icons,
    git_theme: &GitTheme,
    max_cap: Option<usize>,
) -> String {
    let owner_cache = OwnerCache::default();
    let term_width = terminal_size().map(|(w, _)| w.0 as usize);
    let hyperlink = flags.hyperlink == HyperlinkOption::Always;

    // render each top-level meta as its own subtree block (Vec<String> of lines)
    let blocks: Vec<Vec<String>> = metas
        .iter()
        .map(|meta| {
            let single = std::slice::from_ref(meta);
            let padding_rules = get_padding_rules(single, flags);

            let mut grid = Grid::new(GridOptions {
                filling: Filling::Spaces(1),
                direction: Direction::LeftToRight,
            });

            for cell in inner_display_tree(
                single,
                &owner_cache,
                flags,
                colors,
                icons,
                git_theme,
                (0, ""),
                &padding_rules,
                0,
                max_cap,
            ) {
                grid.add(cell);
            }

            // blocks.len() is 1 per tree() precondition
            grid.fit_into_columns(1)
                .to_string()
                .lines()
                .map(str::to_string)
                .collect()
        })
        .collect();

    if blocks.is_empty() {
        return String::new();
    }

    // measure each block's visible width (max visible width across its lines)
    let widths: Vec<usize> = blocks
        .iter()
        .map(|lines| {
            lines
                .iter()
                .map(|line| get_visible_width(line, hyperlink))
                .max()
                .unwrap_or(0)
        })
        .collect();

    // greedy pack: each row contains as many consecutive blocks as fit in term_width.
    // when term_width is unknown, put all blocks on one row.
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut current_row: Vec<usize> = Vec::new();
    let mut current_width: usize = 0;

    for (idx, &w) in widths.iter().enumerate() {
        let added_width = if current_row.is_empty() {
            w
        } else {
            TREE_COLUMN_GUTTER + w
        };
        let fits = match term_width {
            Some(tw) => current_row.is_empty() || current_width + added_width <= tw,
            None => true,
        };
        if fits {
            current_row.push(idx);
            current_width += added_width;
        } else {
            rows.push(std::mem::take(&mut current_row));
            current_row.push(idx);
            current_width = w;
        }
    }
    if !current_row.is_empty() {
        rows.push(current_row);
    }

    // for each row, pad each block's lines to its width, pad shorter blocks with
    // blank lines up to tallest in row, then interleave horizontally.
    let mut output = String::new();
    let gutter = " ".repeat(TREE_COLUMN_GUTTER);

    for row in rows {
        let row_height = row.iter().map(|&i| blocks[i].len()).max().unwrap_or(0);

        for line_idx in 0..row_height {
            for (pos, &block_idx) in row.iter().enumerate() {
                if pos > 0 {
                    output.push_str(&gutter);
                }
                let block = &blocks[block_idx];
                let w = widths[block_idx];
                let line = block.get(line_idx).map(String::as_str).unwrap_or("");
                output.push_str(line);
                let line_visible = get_visible_width(line, hyperlink);
                if line_visible < w {
                    output.push_str(&" ".repeat(w - line_visible));
                }
            }
            output.push('\n');
        }
    }

    output
}

// render just the root meta's name line (no children, no connectors), used as a
// header above horizontally packed subtrees when tree_columns() unwraps a single
// top-level directory. blocks is enforced len==1 by the caller.
fn render_tree_root_header(
    meta: &Meta,
    flags: &Flags,
    colors: &Colors,
    icons: &Icons,
    git_theme: &GitTheme,
) -> String {
    let single = std::slice::from_ref(meta);
    let padding_rules = get_padding_rules(single, flags);
    let owner_cache = OwnerCache::default();
    get_output(
        meta,
        &owner_cache,
        colors,
        icons,
        git_theme,
        flags,
        &DisplayOption::FileName,
        &padding_rules,
        (0, ""),
    )
    .into_iter()
    .next()
    .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)] // should wrap flags, colors, icons, git_theme into one struct
fn inner_display_grid(
    display_option: &DisplayOption,
    metas: &[Meta],
    owner_cache: &OwnerCache,
    flags: &Flags,
    colors: &Colors,
    icons: &Icons,
    git_theme: &GitTheme,
    depth: usize,
    term_width: Option<usize>,
) -> String {
    let mut output = String::new();
    let mut cells = Vec::new();

    let padding_rules = get_padding_rules(metas, flags);
    let mut grid = match flags.layout {
        Layout::OneLine => Grid::new(GridOptions {
            filling: Filling::Spaces(1),
            direction: Direction::LeftToRight,
        }),
        _ => Grid::new(GridOptions {
            filling: Filling::Spaces(2),
            direction: Direction::TopToBottom,
        }),
    };

    // The first iteration (depth == 0) corresponds to the inputs given by the
    // user. We defer displaying directories given by the user unless we've been
    // asked to display the directory itself (rather than its contents).
    let skip_dirs = (depth == 0) && (flags.display != Display::DirectoryOnly);

    // print the files first.
    for meta in metas {
        // Maybe skip showing the directory meta now; show its contents later.
        if skip_dirs
            && (matches!(meta.file_type, FileType::Directory { .. })
                || (matches!(meta.file_type, FileType::SymLink { is_dir: true }))
                    && flags.blocks.0.len() == 1)
        {
            continue;
        }

        let blocks = get_output(
            meta,
            owner_cache,
            colors,
            icons,
            git_theme,
            flags,
            display_option,
            &padding_rules,
            (0, ""),
        );

        for block in blocks {
            cells.push(Cell {
                width: get_visible_width(&block, flags.hyperlink == HyperlinkOption::Always),
                contents: block,
            });
        }
    }

    // Print block headers
    if flags.header.0 && flags.layout == Layout::OneLine && !cells.is_empty() {
        add_header(flags, &cells, &mut grid);
    }

    for cell in cells {
        grid.add(cell);
    }

    let grid_str = if flags.layout == Layout::Grid {
        let effective_width = if depth > 0 {
            let content_indent = (depth + 1) * INDENT_STEP;
            term_width.map(|w| w.saturating_sub(content_indent))
        } else {
            term_width
        };
        if let Some(tw) = effective_width {
            if let Some(gridded_output) = grid.fit_into_width(tw) {
                gridded_output.to_string()
            } else {
                grid.fit_into_columns(1).to_string()
            }
        } else {
            grid.fit_into_columns(1).to_string()
        }
    } else {
        grid.fit_into_columns(flags.blocks.0.len()).to_string()
    };

    if depth > 0 {
        let content_prefix = " ".repeat((depth + 1) * INDENT_STEP);
        let has_trailing_newline = grid_str.ends_with('\n');
        let mut indented =
            String::with_capacity(grid_str.len() + grid_str.lines().count() * content_prefix.len());
        for (i, line) in grid_str.lines().enumerate() {
            if i > 0 {
                indented.push('\n');
            }
            if !line.is_empty() {
                indented.push_str(&content_prefix);
            }
            indented.push_str(line);
        }
        if has_trailing_newline {
            indented.push('\n');
        }
        output += &indented;
    } else {
        output += &grid_str;
    }

    let should_display_folder_path = should_display_folder_path(depth, metas);

    // print the folder content
    for meta in metas {
        if let Some(content) = &meta.content {
            if should_display_folder_path {
                output.truncate(output.trim_end_matches('\n').len());
                output.push('\n');
                output += &display_folder_path(meta, depth);
            }

            let display_option = DisplayOption::Relative {
                base_path: &meta.path,
            };

            output += &inner_display_grid(
                &display_option,
                content,
                owner_cache,
                flags,
                colors,
                icons,
                git_theme,
                depth + 1,
                term_width,
            );
        }
    }

    output
}

fn add_header(flags: &Flags, cells: &[Cell], grid: &mut Grid) {
    let num_columns: usize = flags.blocks.0.len();

    let mut widths = flags
        .blocks
        .0
        .iter()
        .map(|b| get_visible_width(b.get_header(), flags.hyperlink == HyperlinkOption::Always))
        .collect::<Vec<usize>>();

    // find max widths of each column
    for (index, cell) in cells.iter().enumerate() {
        let index = index % num_columns;
        widths[index] = std::cmp::max(widths[index], cell.width);
    }

    for (idx, block) in flags.blocks.0.iter().enumerate() {
        // center and underline header
        let underlined_header = crossterm::style::Stylize::attribute(
            format!("{: ^1$}", block.get_header(), widths[idx]),
            crossterm::style::Attribute::Underlined,
        )
        .to_string();

        grid.add(Cell {
            width: widths[idx],
            contents: underlined_header,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn inner_display_tree(
    metas: &[Meta],
    owner_cache: &OwnerCache,
    flags: &Flags,
    colors: &Colors,
    icons: &Icons,
    git_theme: &GitTheme,
    tree_depth_prefix: (usize, &str),
    padding_rules: &HashMap<Block, usize>,
    tree_index: usize,
    max_cap: Option<usize>,
) -> Vec<Cell> {
    let mut cells = Vec::new();

    // apply tree filter: dirs always shown, non-dirs must match a glob
    let filtered: Vec<&Meta> = if !flags.tree_filter.0.is_empty() {
        metas
            .iter()
            .filter(|m| {
                matches!(m.file_type, FileType::Directory { .. })
                    || matches!(m.file_type, FileType::SymLink { is_dir: true })
                    || flags.tree_filter.0.is_match(m.name.file_name())
            })
            .collect()
    } else {
        metas.iter().collect()
    };

    // truncate to the resolved max cap (handles both MaxShown::Count and Auto)
    let (display_metas, truncated) = if let Some(n) = max_cap {
        if filtered.len() > n {
            (&filtered[..n], filtered.len() - n)
        } else {
            (filtered.as_slice(), 0usize)
        }
    } else {
        (filtered.as_slice(), 0usize)
    };

    let last_idx = display_metas.len();

    for (idx, meta) in display_metas.iter().enumerate() {
        let is_last = truncated == 0 && idx + 1 == last_idx;
        let current_prefix = if tree_depth_prefix.0 > 0 {
            if !is_last {
                format!("{}{} ", tree_depth_prefix.1, EDGE)
            } else {
                format!("{}{} ", tree_depth_prefix.1, CORNER)
            }
        } else {
            tree_depth_prefix.1.to_string()
        };

        for block in get_output(
            meta,
            owner_cache,
            colors,
            icons,
            git_theme,
            flags,
            &DisplayOption::FileName,
            padding_rules,
            (tree_index, &current_prefix),
        ) {
            cells.push(Cell {
                width: get_visible_width(&block, flags.hyperlink == HyperlinkOption::Always),
                contents: block,
            });
        }

        if let Some(content) = &meta.content {
            let new_prefix = if tree_depth_prefix.0 > 0 {
                if !is_last {
                    format!("{}{} ", tree_depth_prefix.1, LINE)
                } else {
                    format!("{}{} ", tree_depth_prefix.1, BLANK)
                }
            } else {
                tree_depth_prefix.1.to_string()
            };

            cells.extend(inner_display_tree(
                content,
                owner_cache,
                flags,
                colors,
                icons,
                git_theme,
                (tree_depth_prefix.0 + 1, &new_prefix),
                padding_rules,
                tree_index,
                max_cap,
            ));
        }
    }

    if truncated > 0 {
        let prefix = if tree_depth_prefix.0 > 0 {
            format!("{}{} ", tree_depth_prefix.1, CORNER)
        } else {
            tree_depth_prefix.1.to_string()
        };
        let summary_text = format!("{}... and {} more", prefix, truncated);
        let colored_summary = colors.colorize(&summary_text, &Elem::TreeEdge).to_string();

        for i in 0..flags.blocks.0.len() {
            if i == tree_index {
                cells.push(Cell {
                    width: get_visible_width(
                        &colored_summary,
                        flags.hyperlink == HyperlinkOption::Always,
                    ),
                    contents: colored_summary.clone(),
                });
            } else {
                cells.push(Cell {
                    width: 0,
                    contents: String::new(),
                });
            }
        }
    }

    cells
}

fn should_display_folder_path(depth: usize, metas: &[Meta]) -> bool {
    if depth > 0 {
        true
    } else {
        let folder_number = metas
            .iter()
            .filter(|x| {
                matches!(x.file_type, FileType::Directory { .. })
                    || (matches!(x.file_type, FileType::SymLink { is_dir: true }))
            })
            .count();

        folder_number > 1 || folder_number < metas.len()
    }
}

fn display_folder_path(meta: &Meta, depth: usize) -> String {
    let indent = " ".repeat((depth + 1) * INDENT_STEP);
    format!("{indent}{}:\n", meta.path.to_string_lossy())
}

#[allow(clippy::too_many_arguments)]
fn get_output(
    meta: &Meta,
    owner_cache: &OwnerCache,
    colors: &Colors,
    icons: &Icons,
    git_theme: &GitTheme,
    flags: &Flags,
    display_option: &DisplayOption,
    padding_rules: &HashMap<Block, usize>,
    tree: (usize, &str),
) -> Vec<String> {
    let mut strings: Vec<String> = Vec::new();
    let colorize_missing = |string: &str| colors.colorize(string, &Elem::NoAccess);

    for (i, block) in flags.blocks.0.iter().enumerate() {
        let mut block_vec = if Layout::Tree == flags.layout && tree.0 == i {
            vec![colors.colorize(tree.1, &Elem::TreeEdge)]
        } else {
            Vec::new()
        };

        match block {
            Block::INode => block_vec.push(match &meta.inode {
                Some(inode) => inode.render(colors),
                None => colorize_missing("?"),
            }),
            Block::Links => block_vec.push(match &meta.links {
                Some(links) => links.render(colors),
                None => colorize_missing("?"),
            }),
            Block::Permission => {
                block_vec.extend([
                    meta.file_type.render(colors),
                    match &meta.permissions_or_attributes {
                        Some(permissions_or_attributes) => {
                            permissions_or_attributes.render(colors, flags)
                        }
                        None => colorize_missing("?????????"),
                    },
                    match &meta.access_control {
                        Some(access_control) => access_control.render_method(colors),
                        None => colorize_missing(""),
                    },
                ]);
            }
            Block::User => block_vec.push(match &meta.owner {
                Some(owner) => owner.render_user(colors, owner_cache, flags),
                None => colorize_missing("?"),
            }),
            Block::Group => block_vec.push(match &meta.owner {
                Some(owner) => owner.render_group(colors, owner_cache, flags),
                None => colorize_missing("?"),
            }),
            Block::Context => block_vec.push(match &meta.access_control {
                Some(access_control) => access_control.render_context(colors),
                None => colorize_missing("?"),
            }),
            Block::Size => {
                let pad = if Layout::Tree == flags.layout && 0 == tree.0 && 0 == i {
                    None
                } else {
                    Some(padding_rules[&Block::SizeValue])
                };
                block_vec.push(match &meta.size {
                    Some(size) => size.render(colors, flags, pad),
                    None => colorize_missing("?"),
                })
            }
            Block::SizeValue => block_vec.push(match &meta.size {
                Some(size) => size.render_value(colors, flags),
                None => colorize_missing("?"),
            }),
            Block::Date => block_vec.push(match &meta.date {
                Some(date) => date.render(colors, flags),
                None => colorize_missing("?"),
            }),
            Block::Name => {
                block_vec.extend([
                    meta.name.render(
                        colors,
                        icons,
                        display_option,
                        flags.hyperlink,
                        flags.literal.0,
                    ),
                    meta.indicator.render(flags),
                ]);
                if !(flags.no_symlink.0 || flags.dereference.0 || flags.layout == Layout::Grid) {
                    block_vec.push(meta.symlink.render(colors, flags))
                }
            }
            Block::GitStatus => {
                if let Some(_s) = &meta.git_status {
                    block_vec.push(_s.render(colors, git_theme));
                }
            }
        };
        strings.push(
            block_vec
                .into_iter()
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
                .join(""),
        );
    }
    strings
}

fn get_visible_width(input: &str, hyperlink: bool) -> usize {
    let mut nb_invisible_char = 0;

    // If the input has color, do not compute the length contributed by the color to the actual length
    for (idx, _) in input.match_indices("\u{1b}[") {
        let (_, s) = input.split_at(idx);

        let m_pos = s.find('m');
        if let Some(len) = m_pos {
            // len points to the 'm' character, we must include 'm' to invisible characters
            nb_invisible_char += len + 1;
        }
    }

    if hyperlink {
        for (idx, _) in input.match_indices("\x1B]8;;") {
            let (_, s) = input.split_at(idx);

            let m_pos = s.find("\x1B\x5C");
            if let Some(len) = m_pos {
                // len points to the '\x1B' character, we must include both '\x1B' and '\x5C' to invisible characters
                nb_invisible_char += len + 2
            }
        }
    }

    // `UnicodeWidthStr::width` counts all unicode characters including escape '\u{1b}' and hyperlink '\x1B'
    UnicodeWidthStr::width(input) - nb_invisible_char
}

fn detect_size_lengths(metas: &[Meta], flags: &Flags) -> usize {
    let mut max_value_length: usize = 0;

    for meta in metas {
        let value_len = match &meta.size {
            Some(size) => size.value_string(flags).len(),
            None => 0,
        };

        if value_len > max_value_length {
            max_value_length = value_len;
        }

        if Layout::Tree == flags.layout {
            if let Some(subs) = &meta.content {
                let sub_length = detect_size_lengths(subs, flags);
                if sub_length > max_value_length {
                    max_value_length = sub_length;
                }
            }
        }
    }

    max_value_length
}

fn get_padding_rules(metas: &[Meta], flags: &Flags) -> HashMap<Block, usize> {
    let mut padding_rules: HashMap<Block, usize> = HashMap::new();

    if flags.blocks.0.contains(&Block::Size) {
        let size_val = detect_size_lengths(metas, flags);

        padding_rules.insert(Block::SizeValue, size_val);
    }

    padding_rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::app::Cli;
    use crate::color;
    use crate::color::Colors;
    use crate::flags::{HyperlinkOption, IconOption, IconTheme as FlagTheme, PermissionFlag};
    use crate::icon::Icons;
    use crate::meta::{FileType, Name};
    use crate::{flags, sort};
    use assert_fs::prelude::*;
    use clap::Parser;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn test_display_get_visible_width_without_icons() {
        for (s, l) in [
            ("Ｈｅｌｌｏ,ｗｏｒｌｄ!", 22),
            ("ASCII1234-_", 11),
            ("制作样本。", 10),
            ("日本語", 6),
            ("샘플은 무료로 드리겠습니다", 28),
            ("👩🐩", 4),
            ("🔬", 2),
        ] {
            let path = Path::new(s);
            let name = Name::new(
                path,
                FileType::File {
                    exec: false,
                    uid: false,
                },
            );
            let output = name
                .render(
                    &Colors::new(color::ThemeOption::NoColor),
                    &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
                    &DisplayOption::FileName,
                    HyperlinkOption::Never,
                    false,
                )
                .to_string();

            assert_eq!(get_visible_width(&output, false), l);
        }
    }

    #[test]
    fn test_display_get_visible_width_with_icons() {
        for (s, l) in [
            // Add 3 characters for the icons.
            ("Ｈｅｌｌｏ,ｗｏｒｌｄ!", 24),
            ("ASCII1234-_", 13),
            ("File with space", 19),
            ("制作样本。", 12),
            ("日本語", 8),
            ("샘플은 무료로 드리겠습니다", 30),
            ("👩🐩", 6),
            ("🔬", 4),
        ] {
            let path = Path::new(s);
            let name = Name::new(
                path,
                FileType::File {
                    exec: false,
                    uid: false,
                },
            );
            let output = name
                .render(
                    &Colors::new(color::ThemeOption::NoColor),
                    &Icons::new(false, IconOption::Always, FlagTheme::Fancy, " ".to_string()),
                    &DisplayOption::FileName,
                    HyperlinkOption::Never,
                    false,
                )
                .to_string();

            assert_eq!(get_visible_width(&output, false), l);
        }
    }

    #[test]
    fn test_display_get_visible_width_with_colors() {
        // crossterm implicitly colors if NO_COLOR is set.
        crossterm::style::force_color_output(true);

        for (s, l) in [
            ("Ｈｅｌｌｏ,ｗｏｒｌｄ!", 22),
            ("ASCII1234-_", 11),
            ("File with space", 17),
            ("制作样本。", 10),
            ("日本語", 6),
            ("샘플은 무료로 드리겠습니다", 28),
            ("👩🐩", 4),
            ("🔬", 2),
        ] {
            let path = Path::new(s);
            let name = Name::new(
                path,
                FileType::File {
                    exec: false,
                    uid: false,
                },
            );
            let output = name
                .render(
                    &Colors::new(color::ThemeOption::NoLscolors),
                    &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
                    &DisplayOption::FileName,
                    HyperlinkOption::Never,
                    false,
                )
                .to_string();

            // check if the color is present.
            assert!(
                output.starts_with("\u{1b}[38;5;"),
                "{output:?} should start with color"
            );
            assert!(output.ends_with("[39m"), "reset foreground color");

            assert_eq!(get_visible_width(&output, false), l, "visible match");
        }
    }

    #[test]
    fn test_display_get_visible_width_without_colors() {
        for (s, l) in [
            ("Ｈｅｌｌｏ,ｗｏｒｌｄ!", 22),
            ("ASCII1234-_", 11),
            ("File with space", 17),
            ("制作样本。", 10),
            ("日本語", 6),
            ("샘플은 무료로 드리겠습니다", 28),
            ("👩🐩", 4),
            ("🔬", 2),
        ] {
            let path = Path::new(s);
            let name = Name::new(
                path,
                FileType::File {
                    exec: false,
                    uid: false,
                },
            );
            let output = name
                .render(
                    &Colors::new(color::ThemeOption::NoColor),
                    &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
                    &DisplayOption::FileName,
                    HyperlinkOption::Never,
                    false,
                )
                .to_string();

            // check if the color is present.
            assert!(!output.starts_with("\u{1b}[38;5;"));
            assert!(!output.ends_with("[0m"));

            assert_eq!(get_visible_width(&output, false), l);
        }
    }

    #[test]
    fn test_display_get_visible_width_hypelink_simple() {
        for (s, l) in [
            ("Ｈｅｌｌｏ,ｗｏｒｌｄ!", 22),
            ("ASCII1234-_", 11),
            ("File with space", 15),
            ("制作样本。", 10),
            ("日本語", 6),
            ("샘플은 무료로 드리겠습니다", 26),
            ("👩🐩", 4),
            ("🔬", 2),
        ] {
            // rending name require actual file, so we are mocking that
            let output = format!("\x1B]8;;{}\x1B\x5C{}\x1B]8;;\x1B\x5C", "url://fake-url", s);
            assert_eq!(get_visible_width(&output, true), l);
        }
    }

    fn sort(metas: &mut Vec<Meta>, sorters: &Vec<(flags::SortOrder, sort::SortFn)>) {
        metas.sort_unstable_by(|a, b| sort::by_meta(sorters, a, b));

        for meta in metas {
            if let Some(ref mut content) = meta.content {
                sort(content, sorters);
            }
        }
    }

    #[test]
    fn test_display_tree_with_all() {
        let argv = ["lsd", "--tree", "--all"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("one.d").create_dir_all().unwrap();
        dir.child("one.d/two").touch().unwrap();
        dir.child("one.d/.hidden").touch().unwrap();
        let mut metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        sort(&mut metas, &sort::assemble_sorters(&flags));
        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        assert_eq!("one.d\n├── .hidden\n└── two\n", output);
    }

    /// Different level of folder may form a different width
    /// we must make sure it is aligned in all level
    ///
    /// dir has a bytes size
    /// empty file has an empty size
    /// `---blocks size,name` can help us for this case
    #[test]
    fn test_tree_align_subfolder() {
        let argv = ["lsd", "--tree", "--blocks", "size,name"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("dir").create_dir_all().unwrap();
        dir.child("dir/file").touch().unwrap();
        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        let length_before_b = |i| -> usize {
            output
                .lines()
                .nth(i)
                .unwrap()
                .split(['K', 'B'])
                .next()
                .unwrap()
                .len()
        };
        assert_eq!(length_before_b(0), length_before_b(1));
        assert_eq!(
            output.lines().next().unwrap().find('d'),
            output.lines().nth(1).unwrap().find('└')
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_tree_size_first_without_name() {
        let argv = ["lsd", "--tree", "--blocks", "size,permission"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("dir").create_dir_all().unwrap();
        dir.child("dir/file").touch().unwrap();
        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        assert_eq!(output.lines().nth(1).unwrap().chars().next().unwrap(), '└');
        assert_eq!(
            output
                .lines()
                .next()
                .unwrap()
                .chars()
                .position(|x| x == 'd'),
            output
                .lines()
                .nth(1)
                .unwrap()
                .chars()
                .position(|x| x == '.'),
        );
    }

    #[test]
    fn test_tree_edge_before_name() {
        let argv = ["lsd", "--tree", "--long"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("one.d").create_dir_all().unwrap();
        dir.child("one.d/two").touch().unwrap();
        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        assert!(output.ends_with("└── two\n"));
    }

    #[test]
    fn test_grid_all_block_headers() {
        let argv = [
            "lsd",
            "--header",
            "--blocks",
            "permission,user,group,size,date,name,inode,links",
        ];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("testdir").create_dir_all().unwrap();
        dir.child("test").touch().unwrap();
        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(1, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = grid(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        dir.close().unwrap();

        assert!(output.contains("Permissions"));
        assert!(output.contains("User"));
        assert!(output.contains("Group"));
        assert!(output.contains("Size"));
        assert!(output.contains("Date Modified"));
        assert!(output.contains("Name"));
        assert!(output.contains("INode"));
        assert!(output.contains("Links"));
    }

    #[test]
    fn test_grid_no_header_with_empty_meta() {
        let argv = ["lsd", "--header", "-l"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("testdir").create_dir_all().unwrap();
        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(1, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = grid(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        dir.close().unwrap();

        assert!(!output.contains("Permissions"));
        assert!(!output.contains("User"));
        assert!(!output.contains("Group"));
        assert!(!output.contains("Size"));
        assert!(!output.contains("Date Modified"));
        assert!(!output.contains("Name"));
    }

    #[test]
    fn test_folder_path() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        let file_path = tmp_dir.path().join("file");
        std::fs::File::create(&file_path).expect("failed to create the file");
        let file = Meta::from_path(&file_path, false, PermissionFlag::Rwx).unwrap();

        let dir_path = tmp_dir.path().join("dir");
        std::fs::create_dir(&dir_path).expect("failed to create the dir");
        let dir = Meta::from_path(&dir_path, false, PermissionFlag::Rwx).unwrap();

        assert_eq!(
            display_folder_path(&dir, 0),
            format!(
                "  {}{}dir:\n",
                tmp_dir.path().to_string_lossy(),
                std::path::MAIN_SEPARATOR
            )
        );

        assert_eq!(
            display_folder_path(&dir, 1),
            format!(
                "    {}{}dir:\n",
                tmp_dir.path().to_string_lossy(),
                std::path::MAIN_SEPARATOR
            )
        );

        assert_eq!(
            display_folder_path(&dir, 2),
            format!(
                "      {}{}dir:\n",
                tmp_dir.path().to_string_lossy(),
                std::path::MAIN_SEPARATOR
            )
        );

        const YES: bool = true;
        const NO: bool = false;

        assert_eq!(
            should_display_folder_path(0, &[file.clone()]),
            YES // doesn't matter since there is no folder
        );
        assert_eq!(should_display_folder_path(0, &[dir.clone()]), NO);
        assert_eq!(
            should_display_folder_path(0, &[file.clone(), dir.clone()]),
            YES
        );
        assert_eq!(
            should_display_folder_path(0, &[dir.clone(), dir.clone()]),
            YES
        );
        assert_eq!(
            should_display_folder_path(0, &[file.clone(), file.clone()]),
            YES // doesn't matter since there is no folder
        );

        drop(dir); // to avoid clippy complains about previous .clone()
        drop(file);
    }

    #[cfg(unix)]
    #[test]
    fn test_folder_path_with_links() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        let file_path = tmp_dir.path().join("file");
        std::fs::File::create(&file_path).expect("failed to create the file");
        let file = Meta::from_path(&file_path, false, PermissionFlag::Rwx).unwrap();

        let dir_path = tmp_dir.path().join("dir");
        std::fs::create_dir(&dir_path).expect("failed to create the dir");
        let dir = Meta::from_path(&dir_path, false, PermissionFlag::Rwx).unwrap();

        let link_path = tmp_dir.path().join("link");
        std::os::unix::fs::symlink("dir", &link_path).unwrap();
        let link = Meta::from_path(&link_path, false, PermissionFlag::Rwx).unwrap();

        const YES: bool = true;
        const NO: bool = false;

        assert_eq!(should_display_folder_path(0, &[link.clone()]), NO);

        assert_eq!(
            should_display_folder_path(0, &[file.clone(), link.clone()]),
            YES
        );

        assert_eq!(
            should_display_folder_path(0, &[dir.clone(), link.clone()]),
            YES
        );

        drop(dir); // to avoid clippy complains about previous .clone()
        drop(file);
        drop(link);
    }

    #[test]
    fn test_tree_with_max_shown_truncation() {
        let argv = ["lsd", "--tree", "--max-shown", "2"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("a").touch().unwrap();
        dir.child("b").touch().unwrap();
        dir.child("c").touch().unwrap();
        dir.child("d").touch().unwrap();

        let mut metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        sort(&mut metas, &sort::assemble_sorters(&flags));

        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // first 2 items shown, summary line for remaining 2
        assert!(output.contains("... and 2 more"), "summary line missing");
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3, "expected 2 items + summary line");
    }

    #[test]
    fn test_tree_with_max_shown_auto_uses_top_count() {
        // --max-shown -1 → Auto → cap = top-level entry count.
        // top has 3 items; subdir has 5 → should truncate to 3 with summary line.
        let argv = ["lsd", "--tree", "--max-shown", "-1"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("a.txt").touch().unwrap();
        dir.child("b.txt").touch().unwrap();
        dir.child("sub").create_dir_all().unwrap();
        dir.child("sub/s1").touch().unwrap();
        dir.child("sub/s2").touch().unwrap();
        dir.child("sub/s3").touch().unwrap();
        dir.child("sub/s4").touch().unwrap();
        dir.child("sub/s5").touch().unwrap();

        let mut root = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx).unwrap();
        let (content, _) = root.recurse_into(42, &flags, None).unwrap();
        root.content = content;
        let mut metas = vec![root];
        sort(&mut metas, &sort::assemble_sorters(&flags));

        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // top count is 3 (a.txt, b.txt, sub), so subdir with 5 children truncates to 3 → "... and 2 more"
        assert!(
            output.contains("... and 2 more"),
            "subdir should be truncated to top-level count (3), leaving 2 hidden. output:\n{output}"
        );
        // all 3 top-level entries should still appear (top-level count == cap so nothing truncated there)
        assert!(output.contains("a.txt"), "missing a.txt");
        assert!(output.contains("b.txt"), "missing b.txt");
        assert!(output.contains("sub"), "missing sub");
    }

    #[test]
    fn test_tree_columns_auto_max_shown_satisfies_gate() {
        // --max-shown -1 should count as "set" for the tree-columns gate.
        let argv = ["lsd", "--tree", "--max-shown", "-1", "--tree-columns"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("alpha").create_dir_all().unwrap();
        dir.child("alpha/a1").touch().unwrap();
        dir.child("beta").create_dir_all().unwrap();
        dir.child("beta/b1").touch().unwrap();
        dir.child("gamma").create_dir_all().unwrap();
        dir.child("gamma/g1").touch().unwrap();

        let mut root = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx).unwrap();
        let (content, _) = root.recurse_into(42, &flags, None).unwrap();
        root.content = content;
        let metas = vec![root];

        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // gate passed → children packed horizontally below the root header
        let packed_line = output
            .lines()
            .find(|l| l.contains("alpha") && l.contains("beta") && l.contains("gamma"))
            .unwrap_or_else(|| panic!("expected packed line with all three children. got:\n{output}"));
        assert!(
            !packed_line.is_empty(),
            "packed line should not be empty, got:\n{output}"
        );
    }

    #[test]
    fn test_tree_with_tree_filter() {
        let argv = ["lsd", "--tree", "--tree-filter", "*.rs"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("subdir").create_dir_all().unwrap();
        dir.child("main.rs").touch().unwrap();
        dir.child("readme.md").touch().unwrap();

        let mut metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        sort(&mut metas, &sort::assemble_sorters(&flags));

        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        assert!(output.contains("main.rs"), "matching file should be shown");
        assert!(output.contains("subdir"), "dirs should always be shown");
        assert!(
            !output.contains("readme.md"),
            "non-matching file should be hidden"
        );
    }

    #[test]
    fn test_tree_with_filter_and_max_shown() {
        let argv = ["lsd", "--tree", "--tree-filter", "*.rs", "--max-shown", "1"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("a.rs").touch().unwrap();
        dir.child("b.rs").touch().unwrap();
        dir.child("c.md").touch().unwrap();

        let mut metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        sort(&mut metas, &sort::assemble_sorters(&flags));

        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // c.md excluded by filter; a.rs shown; b.rs in "... and 1 more"
        assert!(
            !output.contains("c.md"),
            "non-matching file should be hidden"
        );
        assert!(
            output.contains("... and 1 more"),
            "summary should reflect filtered count"
        );
    }

    #[test]
    fn test_recursion_indent_depth1() {
        let argv = ["lsd", "--recursive"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("root_file").touch().unwrap();
        dir.child("subdir").create_dir_all().unwrap();
        dir.child("subdir/file.rs").touch().unwrap();

        let mut metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        sort(&mut metas, &sort::assemble_sorters(&flags));

        let output = grid(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // header for depth-1 subdir should be indented 2 spaces
        let header_line = output
            .lines()
            .find(|l| l.contains("subdir") && l.ends_with(':'))
            .expect("header line not found");
        assert!(
            header_line.starts_with("  "),
            "depth-1 header should have 2-space indent, got: {header_line:?}"
        );

        // content under subdir should be indented 4 spaces
        let content_line = output
            .lines()
            .find(|l| l.contains("file.rs"))
            .expect("content line not found");
        assert!(
            content_line.starts_with("    "),
            "depth-1 content should have 4-space indent, got: {content_line:?}"
        );
    }

    #[test]
    fn test_recursion_indent_depth2() {
        let argv = ["lsd", "--recursive"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("subdir/nested").create_dir_all().unwrap();
        dir.child("subdir/nested/deep.rs").touch().unwrap();

        let mut metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        sort(&mut metas, &sort::assemble_sorters(&flags));

        let output = grid(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // depth-2 header should be indented 4 spaces
        let nested_header = output
            .lines()
            .find(|l| l.contains("nested") && l.ends_with(':'))
            .expect("nested header not found");
        assert!(
            nested_header.starts_with("    "),
            "depth-2 header should have 4-space indent, got: {nested_header:?}"
        );

        // content at depth 2 should be indented 6 spaces
        let content_line = output
            .lines()
            .find(|l| l.contains("deep.rs"))
            .expect("deep.rs content not found");
        assert!(
            content_line.starts_with("      "),
            "depth-2 content should have 6-space indent, got: {content_line:?}"
        );
    }

    #[test]
    fn test_recursion_no_double_blank_lines() {
        let argv = ["lsd", "--recursive"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("a_file").touch().unwrap();
        dir.child("subdir").create_dir_all().unwrap();
        dir.child("subdir/b_file").touch().unwrap();

        let mut metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        sort(&mut metas, &sort::assemble_sorters(&flags));

        let output = grid(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // no two consecutive empty lines anywhere in the output
        assert!(
            !output.contains("\n\n\n"),
            "found double blank lines in output:\n{output}"
        );
    }

    #[test]
    fn test_tree_columns_packs_horizontally() {
        let argv = ["lsd", "--tree", "--max-shown", "2", "--tree-columns"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("alpha").create_dir_all().unwrap();
        dir.child("alpha/a1.txt").touch().unwrap();
        dir.child("alpha/a2.txt").touch().unwrap();
        dir.child("beta").create_dir_all().unwrap();
        dir.child("beta/b1.txt").touch().unwrap();
        dir.child("beta/b2.txt").touch().unwrap();

        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // both top-level dir names should appear on the same first line, separated by gutter.
        let first_line = output.lines().next().expect("output has no lines");
        assert!(
            first_line.contains("alpha") && first_line.contains("beta"),
            "first line should contain both top-level dirs, got: {first_line:?}"
        );
    }

    #[test]
    fn test_tree_columns_without_max_shown_falls_back() {
        let argv = ["lsd", "--tree", "--tree-columns"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("alpha").create_dir_all().unwrap();
        dir.child("beta").create_dir_all().unwrap();

        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // fallback is vertical: each dir on its own line.
        let alpha_line = output.lines().find(|l| l.contains("alpha")).unwrap();
        assert!(
            !alpha_line.contains("beta"),
            "fallback should place dirs on separate lines, got: {alpha_line:?}"
        );
    }

    #[test]
    fn test_tree_columns_with_multiple_blocks_falls_back() {
        // --long adds multiple blocks — blocks.len() > 1 triggers fallback.
        let argv = [
            "lsd",
            "--tree",
            "--long",
            "--max-shown",
            "2",
            "--tree-columns",
        ];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("alpha").create_dir_all().unwrap();
        dir.child("beta").create_dir_all().unwrap();

        let metas = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx)
            .unwrap()
            .recurse_into(42, &flags, None)
            .unwrap()
            .0
            .unwrap();
        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        // with --long the two dir names must not appear side-by-side on the same line.
        let alpha_line = output.lines().find(|l| l.contains("alpha")).unwrap();
        assert!(
            !alpha_line.contains("beta"),
            "--long should force vertical fallback, got: {alpha_line:?}"
        );
    }

    #[test]
    fn test_tree_columns_single_dir_packs_children() {
        // mirrors real CLI: core.rs fetch() wraps a single dir arg in one top-level
        // meta whose content holds the children. tree() should unwrap and pack.
        let argv = ["lsd", "--tree", "--max-shown", "2", "--tree-columns"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();
        dir.child("alpha").create_dir_all().unwrap();
        dir.child("alpha/a1.txt").touch().unwrap();
        dir.child("beta").create_dir_all().unwrap();
        dir.child("beta/b1.txt").touch().unwrap();
        dir.child("gamma").create_dir_all().unwrap();
        dir.child("gamma/g1.txt").touch().unwrap();

        let mut root = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx).unwrap();
        let (content, _) = root.recurse_into(42, &flags, None).unwrap();
        root.content = content;
        let metas = vec![root];

        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        let mut lines = output.lines();
        let header = lines.next().expect("output has no header line");
        // header should be the root dir basename only, not any of the children.
        assert!(
            !header.contains("alpha") && !header.contains("beta") && !header.contains("gamma"),
            "header should contain only the root dir name, got: {header:?}"
        );
        // next non-empty line should pack all three child dirs side-by-side.
        let packed_line = lines
            .find(|l| !l.trim().is_empty())
            .expect("no packed line after header");
        assert!(
            packed_line.contains("alpha")
                && packed_line.contains("beta")
                && packed_line.contains("gamma"),
            "packed line should contain all three children, got: {packed_line:?}"
        );
    }

    #[test]
    fn test_tree_columns_single_empty_dir_no_panic() {
        // an empty dir as single arg should not panic — falls through to the
        // len==1 tree_columns path which handles single-block output.
        let argv = ["lsd", "--tree", "--max-shown", "2", "--tree-columns"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let flags = Flags::configure_from(&cli, &Config::with_none()).unwrap();

        let dir = assert_fs::TempDir::new().unwrap();

        let mut root = Meta::from_path(Path::new(dir.path()), false, PermissionFlag::Rwx).unwrap();
        let (content, _) = root.recurse_into(42, &flags, None).unwrap();
        root.content = content;
        let metas = vec![root];

        let output = tree(
            &metas,
            &flags,
            &Colors::new(color::ThemeOption::NoColor),
            &Icons::new(false, IconOption::Never, FlagTheme::Fancy, " ".to_string()),
            &GitTheme::new(),
        );

        assert!(
            !output.is_empty(),
            "empty-dir case should still print the root name"
        );
    }
}
