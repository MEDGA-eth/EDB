//! TUI draw implementation.

use alloy_primitives::U256;
use foundry_compilers::artifacts::sourcemap::SourceElement;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use revm::interpreter::opcode;
use std::{
    collections::{HashSet, VecDeque},
    fmt::Write,
    io,
};

const POPUP_WIDTH: u16 = 60;
const MIN_POPUP_HEIGHT: u16 = 10;

use crate::{
    context::FrontendContext,
    utils::opcode::OpcodeParam,
    window::{PaneFlattened, PaneView, PopupMessage, TerminalMode},
    FrontendTerminal,
};

impl FrontendContext<'_> {
    /// Draws the TUI layout and subcomponents to the given terminal.
    pub(crate) fn draw(&mut self, terminal: &mut FrontendTerminal) -> io::Result<()> {
        terminal.draw(|f| self.draw_layout(f)).map(drop)
    }

    #[inline]
    fn draw_layout(&mut self, f: &mut Frame<'_>) {
        // We need 100 columns to display a 32 byte word in the memory and stack panes.
        let size = f.area();
        let min_width = 100;
        let min_height = 16;
        if size.width < min_width || size.height < min_height {
            self.size_too_small(f, min_width, min_height);
            return;
        }

        // The horizontal layout draws these panes at 50% width.
        let min_column_width_for_horizontal = 200;

        if self.window.use_default_pane {
            if size.width >= min_column_width_for_horizontal {
                self.window.set_large_screen();
            } else {
                self.window.set_small_screen();
            }
        }

        self.screen_layout(f);
    }

    fn size_too_small(&self, f: &mut Frame<'_>, min_width: u16, min_height: u16) {
        let mut lines = Vec::with_capacity(4);

        let l1 = "Terminal size too small:";
        lines.push(Line::from(l1));

        let size = f.area();
        let width_color = if size.width >= min_width { Color::Green } else { Color::Red };
        let height_color = if size.height >= min_height { Color::Green } else { Color::Red };
        let l2 = vec![
            Span::raw("Width = "),
            Span::styled(size.width.to_string(), Style::new().fg(width_color)),
            Span::raw(" Height = "),
            Span::styled(size.height.to_string(), Style::new().fg(height_color)),
        ];
        lines.push(Line::from(l2));

        let l3 = "Needed for current config:";
        lines.push(Line::from(l3));
        let l4 = format!("Width = {min_width} Height = {min_height}");
        lines.push(Line::from(l4));

        let paragraph =
            Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: true });
        f.render_widget(paragraph, size)
    }

    /// Draws the layout in horizontal mode.
    fn screen_layout(&mut self, f: &mut Frame<'_>) {
        let area = f.area();
        let h_height = if self.show_shortcuts { 4 } else { 0 };

        // Split off footer.
        let [app, footer] = Layout::new(
            Direction::Vertical,
            [Constraint::Ratio(100 - h_height, 100), Constraint::Ratio(h_height, 100)],
        )
        .split(area)[..] else {
            unreachable!()
        };

        // update screen size
        self.window.screen_size = app;

        let layout = self.window.get_flattened_layout(app).unwrap();

        if self.show_shortcuts {
            self.draw_footer(f, footer);
        }

        for pane in layout {
            match pane.view {
                PaneView::Memory | PaneView::Calldata | PaneView::Returndata => {
                    self.draw_buffer(f, pane)
                }
                PaneView::Expression => self.draw_expressions(f, pane),
                PaneView::Variable => self.draw_variables(f, pane),
                PaneView::Stack => self.draw_stack(f, pane),
                PaneView::Source => self.draw_src(f, pane),
                PaneView::Trace => self.draw_trace(f, pane),
                PaneView::Opcode => self.draw_op_list(f, pane),
                PaneView::Terminal => self.draw_terminal(f, pane),
                PaneView::Null => self.draw_null(f, pane),
            }
        }

        if let Ok(message) = self.window.get_popup_message() {
            // the background of the popup will take up 4 more columns and 4 more rows than the
            // popup itself
            self.draw_popup(f, area, message);
        }
    }

    fn get_focused_block<'a>(&'a self, pane: &PaneFlattened<'a>) -> Block<'static> {
        // prepare the style
        let (border_style, border_set) = if pane.focused {
            if self.window.editor_mode == TerminalMode::Insert && pane.view == PaneView::Terminal {
                (Style::default().fg(Color::LightGreen), border::DOUBLE)
            } else if pane.view == PaneView::Null {
                (Style::default().fg(Color::LightRed), border::DOUBLE)
            } else {
                (Style::default().fg(Color::Cyan), border::DOUBLE)
            }
        } else {
            (Style::default(), border::PLAIN)
        };

        // prepare the title
        fn get_initials(phrase: &str) -> String {
            phrase
                .split_whitespace() // Split the phrase into words
                .filter_map(|word| word.chars().next()) // Get the first character of each word, if it exists
                .collect() // Collect the characters into a String
        }

        // title format: " [id] > view1 | view2 | view3 | "
        let long_title_n = pane.views.iter().map(|v| v.to_string().len() + 3).sum::<usize>() + 7;
        let view_info: Vec<_> = if pane.views.is_empty() {
            vec![("Empty".to_string(), false)]
        } else if long_title_n < pane.rect.width as usize {
            pane.views.iter().map(|v| (v.to_string(), *v == pane.view)).collect()
        } else {
            pane.views.iter().map(|v| (get_initials(&v.to_string()), *v == pane.view)).collect()
        };

        // prepare the title
        let mut spans = Vec::with_capacity(1 + view_info.len() * 2);
        spans.push(Span::raw(format!(" [{}] > ", pane.id)));
        for (view, is_current_view) in view_info {
            if is_current_view {
                let mut style = Style::default().add_modifier(Modifier::BOLD);
                if pane.focused && pane.views.len() > 1 {
                    style = style.fg(Color::Yellow);
                }
                spans.push(Span::styled(view, style));
            } else {
                spans.push(Span::styled(view, Style::default()));
            }
            spans.push(Span::raw(" | "));
        }

        let mut block = Block::default()
            .style(Style::default())
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_set(border_set)
            .title(Line::from(spans));

        // update bottom right corner with the terminal mode
        if pane.view == PaneView::Terminal {
            match self.window.editor_mode {
                TerminalMode::Insert => {
                    block = block.title_bottom(Line::from(" [ Insert Mode ] ").left_aligned())
                }
                TerminalMode::Normal => {
                    block = block.title_bottom(Line::from(" [ Normal Mode ] ").left_aligned())
                }
            };
        }

        block
    }

    fn draw_terminal<'a>(&'a self, f: &mut Frame<'_>, pane: PaneFlattened<'a>) {
        let block = self.get_focused_block(&pane);

        let (cursor_style, cursor_line_style) = if pane.focused {
            if self.window.editor_mode == TerminalMode::Insert {
                (
                    Style::default()
                        .add_modifier(Modifier::UNDERLINED | Modifier::REVERSED)
                        .fg(Color::LightGreen),
                    Style::default(),
                )
            } else {
                (
                    Style::default().add_modifier(Modifier::REVERSED),
                    Style::default().add_modifier(Modifier::UNDERLINED),
                )
            }
        } else {
            (Style::default(), Style::default())
        };

        let mut editor_mut = self.window.editor.borrow_mut();
        editor_mut.set_cursor_line_style(cursor_line_style);
        editor_mut.set_cursor_style(cursor_style);
        editor_mut.set_block(block);

        // let widget = editor_mut.widget();
        f.render_widget(&*editor_mut, pane.rect);
    }

    fn draw_popup(&self, f: &mut Frame<'_>, area: Rect, msg: PopupMessage) {
        // append a new line before and after the message
        let title = &msg.title;
        let message = format!("\n{}\n", msg.message);

        // prepare the wrap and alignment
        let alignment = Alignment::Left;

        // let's first calcualte the height of the message box
        let msg_width = POPUP_WIDTH;
        let (message, msg_height) = wrap_text(
            &message,
            msg_width as usize,
            MIN_POPUP_HEIGHT as usize,
            Some(&msg.highlights),
            Style::new().bg(Color::LightYellow).fg(Color::Black),
        );

        // Note that the chunk has borders, so we need to add 2 to the width and the height.
        let chunk_width = msg_width + 2;
        let chunk_height = msg_height + 2;

        // then, we try to get the backgroud block (there is 1-line margin)
        let bg_rect = centered_rect(chunk_width + 2, chunk_height + 2, area);
        let bg_block =
            Block::default().borders(Borders::NONE).style(Style::default().bg(Color::DarkGray));
        f.render_widget(bg_block, bg_rect);

        let popup_chunk = Layout::default()
            .direction(Direction::Horizontal)
            .margin(1)
            .constraints([Constraint::Percentage(100)])
            .split(bg_rect)[0];
        if popup_chunk.width != chunk_width || popup_chunk.height != chunk_height {
            panic!(
                "popup_chunk size mismatch: expected ({chunk_width}, {chunk_height}), got ({}, {})",
                popup_chunk.width, popup_chunk.height
            );
        }

        let block = Block::default()
            .title(format!(" [ESC] {title}"))
            .borders(Borders::ALL)
            .style(Style::default().bg(Color::Black).fg(Color::White));

        let paragraph = Paragraph::new(message).block(block).alignment(alignment);
        f.render_widget(paragraph, popup_chunk);
    }

    // TODO
    fn draw_null(&self, f: &mut Frame<'_>, pane: PaneFlattened<'_>) {
        let block = self.get_focused_block(&pane);
        let paragraph = Paragraph::new(Text::from(
            "There is no debug view to show.\n\nPlease try to press CTRL + c to register a view to this pane.".to_string()
        ))
        .block(block)
        .wrap(Wrap { trim: false });
        f.render_widget(paragraph, pane.rect);
    }

    // TODO
    fn draw_trace(&self, f: &mut Frame<'_>, pane: PaneFlattened<'_>) {
        let block = self.get_focused_block(&pane);
        let paragraph =
            Paragraph::new(Text::from("trace displaying under construction".to_string()))
                .block(block)
                .wrap(Wrap { trim: false });
        f.render_widget(paragraph, pane.rect);
    }

    // TODO
    fn draw_variables<'a>(&'a self, f: &mut Frame<'_>, pane: PaneFlattened<'a>) {
        let block = self.get_focused_block(&pane);
        let paragraph =
            Paragraph::new(Text::from("variable displaying under construction".to_string()))
                .block(block)
                .wrap(Wrap { trim: false });
        f.render_widget(paragraph, pane.rect);
    }

    // TODO
    fn draw_expressions<'a>(&'a self, f: &mut Frame<'_>, pane: PaneFlattened<'a>) {
        let block = self.get_focused_block(&pane);
        let paragraph =
            Paragraph::new(Text::from("watcher displaying under construction".to_string()))
                .block(block)
                .wrap(Wrap { trim: false });
        f.render_widget(paragraph, pane.rect);
    }

    fn draw_footer(&self, f: &mut Frame<'_>, area: Rect) {
        let l1 = "[q]: quit | [k/j]: prev/next op | [a/s]: prev/next jump | [c/C]: prev/next call | [g/G]: start/end | [b]: cycle variable/watcher/memory/calldata/returndata/stack";
        let l2 = "[t]: stack labels | [m]: buffer decoding | [shift + k]: cycle trace/source | [ctrl + j/k]: scroll data | ['<char>]: goto breakpoint | [h] toggle help";
        let dimmed = Style::new().add_modifier(Modifier::DIM);
        let lines =
            vec![Line::from(Span::styled(l1, dimmed)), Line::from(Span::styled(l2, dimmed))];
        let paragraph =
            Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: false });
        f.render_widget(paragraph, area);
    }

    fn draw_src<'a>(&'a self, f: &mut Frame<'_>, pane: PaneFlattened<'a>) {
        let (text_output, _) = self.src_text(pane.rect);
        // let call_kind_text = match self.call_kind() {
        //     CallKind::Create | CallKind::Create2 => "Contract creation",
        //     CallKind::Call => "Contract call",
        //     CallKind::StaticCall => "Contract staticcall",
        //     CallKind::CallCode => "Contract callcode",
        //     CallKind::DelegateCall => "Contract delegatecall",
        //     CallKind::AuthCall => "Contract authcall",
        // };
        let block = self.get_focused_block(&pane);
        let paragraph = Paragraph::new(text_output).block(block).wrap(Wrap { trim: false });
        f.render_widget(paragraph, pane.rect);
    }

    fn src_text(&self, area: Rect) -> (Text<'_>, Option<&str>) {
        let (source_element, source_code, source_file) = match self.src_map() {
            Ok(r) => r,
            Err(e) => return (Text::from(e), None),
        };

        // We are handed a vector of SourceElements that give us a span of sourcecode that is
        // currently being executed. This includes an offset and length.
        // This vector is in instruction pointer order, meaning the location of the instruction
        // minus `sum(push_bytes[..pc])`.
        let offset = source_element.offset() as usize;
        let len = source_element.length() as usize;
        let max = source_code.len();

        // Split source into before, relevant, and after chunks, split by line, for formatting.
        let actual_start = offset.min(max);
        let actual_end = (offset + len).min(max);

        let mut before: Vec<_> = source_code[..actual_start].split_inclusive('\n').collect();
        let actual: Vec<_> = source_code[actual_start..actual_end].split_inclusive('\n').collect();
        let mut after: VecDeque<_> = source_code[actual_end..].split_inclusive('\n').collect();

        let num_lines = before.len() + actual.len() + after.len();
        let height = area.height as usize;
        let needed_highlight = actual.len();
        let mid_len = before.len() + actual.len();

        // adjust what text we show of the source code
        let (start_line, end_line) = if needed_highlight > height {
            // highlighted section is more lines than we have available
            let start_line = before.len().saturating_sub(1);
            (start_line, before.len() + needed_highlight)
        } else if height > num_lines {
            // we can fit entire source
            (0, num_lines)
        } else {
            let remaining = height - needed_highlight;
            let mut above = remaining / 2;
            let mut below = remaining / 2;
            if below > after.len() {
                // unused space below the highlight
                above += below - after.len();
            } else if above > before.len() {
                // we have unused space above the highlight
                below += above - before.len();
            } else {
                // no unused space
            }

            // since above is subtracted from before.len(), and the resulting
            // start_line is used to index into before, above must be at least
            // 1 to avoid out-of-range accesses.
            if above == 0 {
                above = 1;
            }
            (before.len().saturating_sub(above), mid_len + below)
        };

        // Unhighlighted line number: gray.
        let u_num = Style::new().fg(Color::Gray);
        // Unhighlighted text: default, dimmed.
        let u_text = Style::new().add_modifier(Modifier::DIM);
        // Highlighted line number: cyan.
        let h_num = Style::new().fg(Color::Cyan);
        // Highlighted text: cyan, bold.
        let h_text = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);

        let mut lines = SourceLines::new(decimal_digits(num_lines));

        // We check if there is other text on the same line before the highlight starts.
        if let Some(last) = before.pop() {
            let last_has_nl = last.ends_with('\n');

            if last_has_nl {
                before.push(last);
            }
            for line in &before[start_line..] {
                lines.push(u_num, line, u_text);
            }

            let first = if !last_has_nl {
                lines.push_raw(h_num, &[Span::raw(last), Span::styled(actual[0], h_text)]);
                1
            } else {
                0
            };

            // Skip the first line if it has already been handled above.
            for line in &actual[first..] {
                lines.push(h_num, line, h_text);
            }
        } else {
            // No text before the current line.
            for line in &actual {
                lines.push(h_num, line, h_text);
            }
        }

        // Fill in the rest of the line as unhighlighted.
        if let Some(last) = actual.last() {
            if !last.ends_with('\n') {
                if let Some(post) = after.pop_front() {
                    if let Some(last) = lines.lines.last_mut() {
                        last.spans.push(Span::raw(post));
                    }
                }
            }
        }

        // Add after highlighted text.
        while mid_len + after.len() > end_line {
            after.pop_back();
        }
        for line in after {
            lines.push(u_num, line, u_text);
        }

        // pad with empty to each line to ensure the previous text is cleared
        for line in &mut lines.lines {
            // note that the \n is not included in the line length
            if area.width as usize > line.width() + 1 {
                line.push_span(Span::raw(" ".repeat(area.width as usize - line.width() - 1)));
            }
        }

        (Text::from(lines.lines), Some(source_file))
    }

    /// Returns source map, source code and source name of the current line.
    fn src_map(&self) -> Result<(SourceElement, &str, &str), String> {
        Err("source code displaying under construction".to_string())

        // let address = self.address();
        // let Some(contract_name) = self.debugger.identified_contracts.get(address) else {
        //     return Err(format!("Unknown contract at address {address}"));
        // };

        // let Some(mut files_source_code) =
        //     self.debugger.contracts_sources.get_sources(contract_name)
        // else {
        //     return Err(format!("No source map index for contract {contract_name}"));
        // };

        // let Some((create_map, rt_map)) = self.debugger.pc_ic_maps.get(contract_name) else {
        //     return Err(format!("No PC-IC maps for contract {contract_name}"));
        // };

        // let is_create = matches!(self.call_kind(), CallKind::Create | CallKind::Create2);
        // let pc = self.current_step().pc;
        // let Some((source_element, source_code, source_file)) =
        //     files_source_code.find_map(|(artifact, source)| {
        //         let bytecode = if is_create {
        //             &artifact.bytecode.bytecode
        //         } else {
        //             artifact.bytecode.deployed_bytecode.bytecode.as_ref()?
        //         };
        //         let source_map = bytecode.source_map()?.expect("failed to parse");

        //         let pc_ic_map = if is_create { create_map } else { rt_map };
        //         let ic = pc_ic_map.get(pc)?;

        //         // Solc indexes source maps by instruction counter, but Vyper indexes by program
        //         // counter.
        //         let source_element = if matches!(source.language, MultiCompilerLanguage::Solc(_))
        // {             source_map.get(ic)?
        //         } else {
        //             source_map.get(pc)?
        //         };
        //         // if the source element has an index, find the sourcemap for that index
        //         let res = source_element
        //             .index()
        //             // if index matches current file_id, return current source code
        //             .and_then(|index| {
        //                 (index == artifact.file_id)
        //                     .then(|| (source_element.clone(), source.source.as_str(),
        // &source.name))             })
        //             .or_else(|| {
        //                 // otherwise find the source code for the element's index
        //                 self.debugger
        //                     .contracts_sources
        //                     .sources_by_id
        //                     .get(&artifact.build_id)?
        //                     .get(&source_element.index()?)
        //                     .map(|source| {
        //                         (source_element.clone(), source.source.as_str(), &source.name)
        //                     })
        //             });

        //         res
        //     })
        // else {
        //     return Err(format!("No source map for contract {contract_name}"));
        // };

        // Ok((source_element, source_code, source_file))
    }

    fn draw_op_list<'a>(&'a self, f: &mut Frame<'_>, pane: PaneFlattened<'a>) {
        let debug_steps = self.debug_steps();
        let max_pc = debug_steps.iter().map(|step| step.pc).max().unwrap_or(0);
        let max_pc_len = hex_digits(max_pc);

        let items = debug_steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let mut content = String::with_capacity(64);
                write!(content, "{:0>max_pc_len$x}|", step.pc).unwrap();
                if let Some(op) = self.opcode_list.get(i) {
                    content.push_str(op);
                }
                ListItem::new(Span::styled(content, Style::new().fg(Color::White)))
            })
            .collect::<Vec<_>>();

        let block = self.get_focused_block(&pane);
        let list = List::new(items)
            .block(block)
            .highlight_symbol("▶")
            .highlight_style(Style::new().fg(Color::White).bg(Color::DarkGray))
            .scroll_padding(1);
        let mut state = ListState::default().with_selected(Some(self.current_step));
        f.render_stateful_widget(list, pane.rect, &mut state);
    }

    fn draw_stack<'a>(&'a self, f: &mut Frame<'_>, pane: PaneFlattened<'a>) {
        let step = self.current_step();
        let stack = &step.stack;

        let min_len = decimal_digits(stack.len()).max(2);

        let params = OpcodeParam::of(step.instruction);

        let text: Vec<Line<'_>> = stack
            .iter()
            .rev()
            .enumerate()
            .skip(self.draw_memory.current_stack_startline)
            .map(|(i, stack_item)| {
                let param = params.iter().find(|param| param.index == i);

                let mut spans = Vec::with_capacity(1 + 32 * 2 + 3);

                // Stack index.
                spans.push(Span::styled(format!("{i:0min_len$}| "), Style::new().fg(Color::White)));

                // Item hex bytes.
                hex_bytes_spans(&stack_item.to_be_bytes::<32>(), &mut spans, |_, _| {
                    if param.is_some() {
                        Style::new().fg(Color::Cyan)
                    } else {
                        Style::new().fg(Color::White)
                    }
                });

                if self.stack_labels {
                    if let Some(param) = param {
                        spans.push(Span::raw("| "));
                        spans.push(Span::raw(param.name));
                    }
                }

                spans.push(Span::raw("\n"));

                Line::from(spans)
            })
            .collect();

        let block = self.get_focused_block(&pane);
        let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });
        f.render_widget(paragraph, pane.rect);
    }

    fn draw_buffer<'a>(&'a self, f: &mut Frame<'_>, pane: PaneFlattened<'a>) {
        let step = self.current_step();
        let buf = match pane.view {
            PaneView::Memory => step.memory.as_ref(),
            PaneView::Calldata => step.calldata.as_ref(),
            PaneView::Returndata => step.returndata.as_ref(),
            _ => unreachable!("other data kinds should be handled elsewhere"),
        };

        let min_len = hex_digits(buf.len());

        // Color memory region based on read/write.
        let mut offset = None;
        let mut size = None;
        let mut write_offset = None;
        let mut write_size = None;
        let mut color = None;
        let stack_len = step.stack.len();
        if stack_len > 0 {
            if let Some(accesses) = get_buffer_accesses(step.instruction, &step.stack) {
                if let Some(read_access) = accesses.read {
                    offset = Some(read_access.1.offset);
                    size = Some(read_access.1.size);
                    color = Some(Color::Cyan);
                }
                if let Some(write_access) = accesses.write {
                    if pane.view == PaneView::Memory {
                        write_offset = Some(write_access.offset);
                        write_size = Some(write_access.size);
                    }
                }
            }
        }

        // color word on previous write op
        // TODO: technically it's possible for this to conflict with the current op, ie, with
        // subsequent MCOPYs, but solc can't seem to generate that code even with high optimizer
        // settings
        if self.current_step > 0 {
            let prev_step = self.current_step - 1;
            let prev_step = &self.debug_steps()[prev_step];
            if let Some(write_access) =
                get_buffer_accesses(prev_step.instruction, &prev_step.stack).and_then(|a| a.write)
            {
                if pane.view == PaneView::Memory {
                    offset = Some(write_access.offset);
                    size = Some(write_access.size);
                    color = Some(Color::Green);
                }
            }
        }

        let height = pane.rect.height as usize;
        let end_line = self.draw_memory.current_buf_startline + height;

        let text: Vec<Line<'_>> = buf
            .chunks(32)
            .enumerate()
            .skip(self.draw_memory.current_buf_startline)
            .take_while(|(i, _)| *i < end_line)
            .map(|(i, buf_word)| {
                let mut spans = Vec::with_capacity(1 + 32 * 2 + 1 + 32 / 4 + 1);

                // Buffer index.
                spans.push(Span::styled(
                    format!("{:0min_len$x}| ", i * 32),
                    Style::new().fg(Color::White),
                ));

                // Word hex bytes.
                hex_bytes_spans(buf_word, &mut spans, |j, _| {
                    let mut byte_color = Color::White;
                    let mut end = None;
                    let idx = i * 32 + j;
                    if let (Some(offset), Some(size), Some(color)) = (offset, size, color) {
                        end = Some(offset + size);
                        if (offset..offset + size).contains(&idx) {
                            // [offset, offset + size] is the memory region to be colored.
                            // If a byte at row i and column j in the memory panel
                            // falls in this region, set the color.
                            byte_color = color;
                        }
                    }
                    if let (Some(write_offset), Some(write_size)) = (write_offset, write_size) {
                        // check for overlap with read region
                        let write_end = write_offset + write_size;
                        if let Some(read_end) = end {
                            let read_start = offset.unwrap();
                            if (write_offset..write_end).contains(&read_end) {
                                // if it contains end, start from write_start up to read_end
                                if (write_offset..read_end).contains(&idx) {
                                    return Style::new().fg(Color::Yellow);
                                }
                            } else if (write_offset..write_end).contains(&read_start) {
                                // otherwise if it contains read start, start from read_start up to
                                // write_end
                                if (read_start..write_end).contains(&idx) {
                                    return Style::new().fg(Color::Yellow);
                                }
                            }
                        }
                        if (write_offset..write_end).contains(&idx) {
                            byte_color = Color::Red;
                        }
                    }

                    Style::new().fg(byte_color)
                });

                if self.buf_utf {
                    spans.push(Span::raw("|"));
                    for utf in buf_word.chunks(4) {
                        if let Ok(utf_str) = std::str::from_utf8(utf) {
                            spans.push(Span::raw(utf_str.replace('\0', ".")));
                        } else {
                            spans.push(Span::raw("."));
                        }
                    }
                }

                spans.push(Span::raw("\n"));

                Line::from(spans)
            })
            .collect();

        let block = self.get_focused_block(&pane);
        let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });
        f.render_widget(paragraph, pane.rect);
    }
}

/// Wrapper around a list of [`Line`]s that prepends the line number on each new line.
struct SourceLines<'a> {
    lines: Vec<Line<'a>>,
    max_line_num: usize,
}

impl<'a> SourceLines<'a> {
    fn new(max_line_num: usize) -> Self {
        Self { lines: Vec::new(), max_line_num }
    }

    fn push(&mut self, line_number_style: Style, line: &'a str, line_style: Style) {
        self.push_raw(line_number_style, &[Span::styled(line, line_style)]);
    }

    fn push_raw(&mut self, line_number_style: Style, spans: &[Span<'a>]) {
        let mut line_spans = Vec::with_capacity(4);

        let line_number =
            format!("{number: >width$} ", number = self.lines.len() + 1, width = self.max_line_num);
        line_spans.push(Span::styled(line_number, line_number_style));

        // Space between line number and line text.
        line_spans.push(Span::raw("  "));

        line_spans.extend_from_slice(spans);

        self.lines.push(Line::from(line_spans));
    }
}

/// Container for buffer access information.
struct BufferAccess {
    offset: usize,
    size: usize,
}

/// Container for read and write buffer access information.
struct BufferAccesses {
    /// The read buffer kind and access information.
    read: Option<(PaneView, BufferAccess)>,
    /// The only mutable buffer is the memory buffer, so don't store the buffer kind.
    write: Option<BufferAccess>,
}

/// The memory_access variable stores the index on the stack that indicates the buffer
/// offset/size accessed by the given opcode:
///   (read buffer, buffer read offset, buffer read size, write memory offset, write memory size)
///   \>= 1: the stack index
///   0: no memory access
///   -1: a fixed size of 32 bytes
///   -2: a fixed size of 1 byte
/// The return value is a tuple about accessed buffer region by the given opcode:
///   (read buffer, buffer read offset, buffer read size, write memory offset, write memory size)
fn get_buffer_accesses(op: u8, stack: &[U256]) -> Option<BufferAccesses> {
    let buffer_access = match op {
        opcode::KECCAK256 | opcode::RETURN | opcode::REVERT => {
            (Some((PaneView::Memory, 1, 2)), None)
        }
        opcode::CALLDATACOPY => (Some((PaneView::Calldata, 2, 3)), Some((1, 3))),
        opcode::RETURNDATACOPY => (Some((PaneView::Returndata, 2, 3)), Some((1, 3))),
        opcode::CALLDATALOAD => (Some((PaneView::Calldata, 1, -1i32)), None),
        opcode::CODECOPY => (None, Some((1, 3))),
        opcode::EXTCODECOPY => (None, Some((2, 4))),
        opcode::MLOAD => (Some((PaneView::Memory, 1, -1i32)), None),
        opcode::MSTORE => (None, Some((1, -1i32))),
        opcode::MSTORE8 => (None, Some((1, -2i32))),
        opcode::LOG0 | opcode::LOG1 | opcode::LOG2 | opcode::LOG3 | opcode::LOG4 => {
            (Some((PaneView::Memory, 1, 2)), None)
        }
        opcode::CREATE | opcode::CREATE2 => (Some((PaneView::Memory, 2, 3)), None),
        opcode::CALL | opcode::CALLCODE => (Some((PaneView::Memory, 4, 5)), None),
        opcode::DELEGATECALL | opcode::STATICCALL => (Some((PaneView::Memory, 3, 4)), None),
        opcode::MCOPY => (Some((PaneView::Memory, 2, 3)), Some((1, 3))),
        _ => Default::default(),
    };

    let stack_len = stack.len();
    let get_size = |stack_index| match stack_index {
        -2 => Some(1),
        -1 => Some(32),
        0 => None,
        1.. => {
            if (stack_index as usize) <= stack_len {
                Some(stack[stack_len - stack_index as usize].saturating_to())
            } else {
                None
            }
        }
        _ => panic!("invalid stack index"),
    };

    if buffer_access.0.is_some() || buffer_access.1.is_some() {
        let (read, write) = buffer_access;
        let read_access = read.and_then(|b| {
            let (buffer, offset, size) = b;
            Some((buffer, BufferAccess { offset: get_size(offset)?, size: get_size(size)? }))
        });
        let write_access = write.and_then(|b| {
            let (offset, size) = b;
            Some(BufferAccess { offset: get_size(offset)?, size: get_size(size)? })
        });
        Some(BufferAccesses { read: read_access, write: write_access })
    } else {
        None
    }
}

/// XXX (ZZ): There is an issue where the content in the previous tick is not cleared, rendering the
/// display incorrect. The root cause is still unknown. Anyone interested in this project can start
/// by trying to fix this issue.
///
/// Here is a dirty workaround to pad space to each line to ensure the previous text is cleared.
/// Since we are doing manual line wrapping, we do not need to count the space taken by `\n`
fn wrap_text<'a>(
    text: &'a str,
    width: usize,
    min_height: usize,
    highlights: Option<&HashSet<String>>,
    highlight_style: Style,
) -> (Vec<Line<'a>>, u16) {
    let mut v = vec![];

    for line in text.lines() {
        if line.is_empty() {
            v.push(Line::raw(format!("{}\n", " ".repeat(width))));
            continue;
        }

        let f = |text| {
            if highlights.map(|h| h.contains(line.trim())).unwrap_or(false) {
                Line::styled(text, highlight_style)
            } else {
                Line::raw(text)
            }
        };

        let mut l = String::new();
        for word in line.split_whitespace() {
            if l.len() + word.len() + 1 == width {
                v.push(f(format!("{l} {word}\n")));
                l.clear();
                continue;
            }
            if l.len() + word.len() + 1 > width {
                v.push(f(format!("{l}{}\n", " ".repeat(width - l.len() % width))));
                l.clear();
            }

            if !l.is_empty() {
                l.push(' ');
            }
            l.push_str(word);
        }

        if !l.is_empty() {
            v.push(f(format!("{}{}\n", l, " ".repeat(width - l.len() % width))));
        }
    }

    for _ in v.len()..min_height {
        v.push(Line::raw(format!("{}\n", " ".repeat(width))));
    }

    let height = v.len() as u16;
    (v, height)
}

fn hex_bytes_spans(bytes: &[u8], spans: &mut Vec<Span<'_>>, f: impl Fn(usize, u8) -> Style) {
    for (i, &byte) in bytes.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(alloy_primitives::hex::encode([byte]), f(i, byte)));
    }
}

/// Returns the number of decimal digits in the given number.
///
/// This is the same as `n.to_string().len()`.
fn decimal_digits(n: usize) -> usize {
    n.checked_ilog10().unwrap_or(0) as usize + 1
}

/// Returns the number of hexadecimal digits in the given number.
///
/// This is the same as `format!("{n:x}").len()`.
fn hex_digits(n: usize) -> usize {
    n.checked_ilog(16).unwrap_or(0) as usize + 1
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
fn centered_rect(len_x: u16, len_y: u16, r: Rect) -> Rect {
    // Cut the given rectangle into three vertical pieces
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(len_y), Constraint::Fill(1)])
        .split(r);

    // Then cut the middle vertical piece into three width-wise pieces
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Length(len_x), Constraint::Fill(1)])
        .split(popup_layout[1])[1] // Return the middle chunk
}

#[cfg(test)]
mod tests {
    #[test]
    fn decimal_digits() {
        assert_eq!(super::decimal_digits(0), 1);
        assert_eq!(super::decimal_digits(1), 1);
        assert_eq!(super::decimal_digits(2), 1);
        assert_eq!(super::decimal_digits(9), 1);
        assert_eq!(super::decimal_digits(10), 2);
        assert_eq!(super::decimal_digits(11), 2);
        assert_eq!(super::decimal_digits(50), 2);
        assert_eq!(super::decimal_digits(99), 2);
        assert_eq!(super::decimal_digits(100), 3);
        assert_eq!(super::decimal_digits(101), 3);
        assert_eq!(super::decimal_digits(201), 3);
        assert_eq!(super::decimal_digits(999), 3);
        assert_eq!(super::decimal_digits(1000), 4);
        assert_eq!(super::decimal_digits(1001), 4);
    }

    #[test]
    fn hex_digits() {
        assert_eq!(super::hex_digits(0), 1);
        assert_eq!(super::hex_digits(1), 1);
        assert_eq!(super::hex_digits(2), 1);
        assert_eq!(super::hex_digits(9), 1);
        assert_eq!(super::hex_digits(10), 1);
        assert_eq!(super::hex_digits(11), 1);
        assert_eq!(super::hex_digits(15), 1);
        assert_eq!(super::hex_digits(16), 2);
        assert_eq!(super::hex_digits(17), 2);
        assert_eq!(super::hex_digits(0xff), 2);
        assert_eq!(super::hex_digits(0x100), 3);
        assert_eq!(super::hex_digits(0x101), 3);
    }
}
