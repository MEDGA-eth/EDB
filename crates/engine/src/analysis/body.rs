// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use foundry_compilers::artifacts::{BlockOrStatement, Statement};
use serde::{Deserialize, Serialize};

use crate::analysis::{Analyzer, SourceRange};

/// A body containing a single statement without a wrapping block.
/// This struct is meant to capture loop or if statements with single statement as their bodies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatementBody {
    /// The source range of the body.
    pub range: SourceRange,
}

impl StatementBody {
    /// Creates a new statement body.
    pub fn new(range: SourceRange) -> Self {
        Self { range }
    }
}

/// Creates a source range for a [`BlockOrStatement`]. This function will regulate the range by
/// including the semicolon of statements.
pub(super) fn block_or_statement_rage(
    source: &str,
    block_or_statement: &BlockOrStatement,
) -> SourceRange {
    match block_or_statement {
        BlockOrStatement::Block(block) => block.src.into(),
        BlockOrStatement::Statement(statement) => statement_range(source, statement),
    }
}

/// Creates a source range for a [`Statement`]. This function will regulate the range by including
/// the semicolon of statements.
pub(super) fn statement_range(source: &str, stmt: &Statement) -> SourceRange {
    macro_rules! expand_to_semicolon {
        ($stmt:expr) => {{
            let range: SourceRange = $stmt.src.into();
            let range = range.expand_to_next_semicolon(source);
            range
        }};
    }
    match stmt {
        Statement::Block(block) => block.src.into(),
        Statement::Break(stmt) => expand_to_semicolon!(stmt),
        Statement::Continue(stmt) => expand_to_semicolon!(stmt),
        Statement::DoWhileStatement(stmt) => stmt.src.into(),
        Statement::EmitStatement(stmt) => expand_to_semicolon!(stmt),
        Statement::ExpressionStatement(stmt) => expand_to_semicolon!(stmt),
        Statement::ForStatement(stmt) => {
            let range: SourceRange = stmt.src.into();
            let body_range = block_or_statement_rage(source, &stmt.body);
            range.merge(body_range)
        }
        Statement::IfStatement(stmt) => {
            let range: SourceRange = stmt.src.into();
            if let Some(false_body) = &stmt.false_body {
                let body_range = block_or_statement_rage(source, false_body);
                range.merge(body_range)
            } else {
                let body_range = block_or_statement_rage(source, &stmt.true_body);
                range.merge(body_range)
            }
        }
        Statement::InlineAssembly(stmt) => stmt.src.into(),
        Statement::PlaceholderStatement(stmt) => stmt.src.into(),
        Statement::Return(stmt) => expand_to_semicolon!(stmt),
        Statement::RevertStatement(stmt) => expand_to_semicolon!(stmt),
        Statement::TryStatement(stmt) => stmt.src.into(),
        Statement::UncheckedBlock(stmt) => stmt.src.into(),
        Statement::VariableDeclarationStatement(stmt) => expand_to_semicolon!(stmt),
        Statement::WhileStatement(stmt) => {
            let range: SourceRange = stmt.src.into();
            let body_range = block_or_statement_rage(source, &stmt.body);
            range.merge(body_range)
        }
    }
}

impl Analyzer {
    /// Collect single statement bodies in [`BlockOrStatement`]. If it is a block or control flow
    /// statement, it will be skipped. Control flow statements (if/for/while/do-while) are
    /// visited separately and collect their own bodies, so we don't collect them here to avoid
    /// duplicates.
    pub(super) fn collect_statement_bodies(&mut self, body: &BlockOrStatement) {
        let range = match body {
            BlockOrStatement::Statement(statement) => match statement {
                // Skip blocks - they don't need wrapping
                Statement::Block(_) | Statement::UncheckedBlock(_) => return,
                _ => statement_range(&self.source, statement),
            },
            BlockOrStatement::Block(_) => return,
        };
        self.statement_bodies.push(StatementBody::new(range));
    }
}

#[cfg(test)]
mod tests {
    use crate::analysis::tests::compile_and_analyze;

    #[test]
    fn test_collect_statement_bodies_for_loop() {
        let source = r#"
        contract C {
            function a() public {
                for (uint256 i = 0; i < 10; i++)
                    return;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        assert_eq!(analysis.statement_bodies.len(), 1);
        // Check the body is "return;"
        let body0 = &analysis.statement_bodies[0];
        assert_eq!(body0.range.slice_source(source).trim(), "return;");
    }

    #[test]
    fn test_collect_statement_bodies_if_statement() {
        let source = r#"
        contract C {
            function a() public {
                if (true)
                    return;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        assert_eq!(analysis.statement_bodies.len(), 1);
        // Check the body is "return;"
        let body0 = &analysis.statement_bodies[0];
        assert_eq!(body0.range.slice_source(source).trim(), "return;");
    }

    #[test]
    fn test_collect_statement_bodies_if_else_statement() {
        let source = r#"
        contract C {
            function a() public {
                if (true)
                    return;
                else
                    revert();
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        // Both true and false bodies should be collected
        assert_eq!(analysis.statement_bodies.len(), 2);
        let body0 = &analysis.statement_bodies[0];
        let body1 = &analysis.statement_bodies[1];
        assert_eq!(body0.range.slice_source(source).trim(), "return;");
        assert_eq!(body1.range.slice_source(source).trim(), "revert();");
    }

    #[test]
    fn test_collect_statement_bodies_while_loop() {
        let source = r#"
        contract C {
            function a() public {
                while (true)
                    return;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        assert_eq!(analysis.statement_bodies.len(), 1);
        let body0 = &analysis.statement_bodies[0];
        assert_eq!(body0.range.slice_source(source).trim(), "return;");
    }

    #[test]
    fn test_collect_statement_bodies_nested() {
        let source = r#"
        contract C {
            function a() public {
                if (true)
                    for (uint256 i = 0; i < 10; i++)
                        return;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        // The if body (for loop) and the for statement itself
        assert_eq!(analysis.statement_bodies.len(), 2);
    }

    #[test]
    fn test_collect_statement_bodies_skips_blocks() {
        let source = r#"
        contract C {
            function a() public {
                if (true) {
                    return;
                }
                for (uint256 i = 0; i < 10; i++) {
                    return;
                }
                while (true) {
                    return;
                }
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        // No statement bodies should be collected since all are blocks
        assert_eq!(analysis.statement_bodies.len(), 0);
    }

    #[test]
    fn test_collect_statement_bodies_mixed() {
        let source = r#"
        contract C {
            function a() public {
                if (true)
                    return;
                else {
                    revert();
                }
                for (uint256 i = 0; i < 10; i++) {
                    continue;
                }
                while (false)
                    break;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        // Only if true body and while body should be collected (2 statement bodies)
        assert_eq!(analysis.statement_bodies.len(), 2);
    }

    #[test]
    fn test_collect_statement_bodies_else_if() {
        let source = r#"
        contract C {
            function a() public {
                if (true)
                    return;
                else if (false)
                    revert();
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        assert_eq!(analysis.statement_bodies.len(), 3);

        // Body 0: return;
        // Body 1: the if (false) revert(); statement (needs wrapping)
        // Body 2: revert;
        let body0 = &analysis.statement_bodies[0];
        let body1 = &analysis.statement_bodies[1];
        let body2 = &analysis.statement_bodies[2];

        assert_eq!(body0.range.slice_source(source).trim(), "return;");
        assert!(body1.range.slice_source(source).contains("if (false)"));
        assert_eq!(body2.range.slice_source(source).trim(), "revert();");
    }

    #[test]
    fn test_collect_statement_bodies_nested_control_flow_as_body() {
        let source = r#"
        contract C {
            function a() public {
                if (true)
                    for (uint256 i = 0; i < 10; i++)
                        return;
                else
                    while (true)
                        break;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        assert_eq!(analysis.statement_bodies.len(), 4);

        // Verify each body contains the expected content (order may vary)
        let body_texts: Vec<String> = analysis
            .statement_bodies
            .iter()
            .map(|b| b.range.slice_source(source).trim().to_string())
            .collect();

        // Check we have all expected bodies
        assert!(body_texts.iter().any(|t| t.contains("for (uint256 i")));
        assert!(body_texts.iter().any(|t| t == "return;"));
        assert!(body_texts.iter().any(|t| t.contains("while (true)")));
        assert!(body_texts.iter().any(|t| t == "break;"));
    }

    #[test]
    fn test_collect_statement_bodies_deeply_nested_control_flow() {
        let source = r#"
        contract C {
            function a() public {
                if (true)
                    if (false)
                        for (uint256 i = 0; i < 10; i++)
                            while (true)
                                return;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        assert_eq!(analysis.statement_bodies.len(), 4);

        // Verify each body contains the expected content (order may vary)
        let body_texts: Vec<String> = analysis
            .statement_bodies
            .iter()
            .map(|b| b.range.slice_source(source).trim().to_string())
            .collect();

        // Check we have all expected bodies
        assert!(body_texts.iter().any(|t| t.contains("if (false)")));
        assert!(body_texts.iter().any(|t| t.contains("for (uint256 i")));
        assert!(body_texts.iter().any(|t| t.contains("while (true)")));
        assert!(body_texts.iter().any(|t| t == "return;"));
    }
}
