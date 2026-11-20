use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use rade::compiler::{Compiler, OutputFormat};
use rade::runtime::{sha1, sha1_str, ViewCache, ViewEngine, ViewError, ViewMode};

/// Helper function to create an isolated temporary directory within target/test_scratch
fn create_test_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_scratch")
        .join(name);
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir).expect("Failed to create test directory");
    dir
}

/// Validates that generated RHP code conforms 100% to RHP language specifications.
fn verify_rhp_syntax(rhp_code: &str, format: OutputFormat) {
    let trimmed = rhp_code.trim();
    match format {
        OutputFormat::Closure => {
            assert!(
                trimmed.starts_with("return function(dict $__data): string {"),
                "Closure format must start with standard closure signature"
            );
            assert!(
                trimmed.ends_with("};"),
                "Closure format must end with closing closure brace and semicolon"
            );
            assert!(
                trimmed.contains("    $__out = \"\";"),
                "Closure body must initialize $__out accumulator"
            );
            assert!(
                trimmed.contains("    return $__out;"),
                "Closure body must return $__out"
            );
        }
        OutputFormat::Standalone => {
            assert!(
                trimmed.starts_with("$__out = \"\";"),
                "Standalone format must initialize $__out accumulator at the top"
            );
            assert!(
                trimmed.ends_with("return $__out;"),
                "Standalone format must end with return $__out;"
            );
        }
        OutputFormat::DirectivesOnly => {}
    }

    // Must NEVER contain PHP legacy constructs or buffer functions
    assert!(!rhp_code.contains("echo "), "Must not contain legacy 'echo '");
    assert!(!rhp_code.contains("echo("), "Must not contain legacy 'echo('");
    assert!(!rhp_code.contains("ob_start"), "Must not contain ob_start");
    assert!(!rhp_code.contains("ob_get_clean"), "Must not contain ob_get_clean");
    assert!(!rhp_code.contains("ob_end_clean"), "Must not contain ob_end_clean");
    assert!(!rhp_code.contains("ob_get_contents"), "Must not contain ob_get_contents");
    assert!(!rhp_code.contains(".="), "Must use '+=' instead of PHP '.=' for string concatenation");

    // Verify balanced curly braces
    let mut brace_depth = 0;
    let mut in_string = false;
    let mut escaped = false;

    for ch in rhp_code.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if !in_string {
            if ch == '{' {
                brace_depth += 1;
            } else if ch == '}' {
                assert!(brace_depth > 0, "Unmatched closing brace '}}' found in RHP output");
                brace_depth -= 1;
            }
        }
    }

    assert_eq!(brace_depth, 0, "Unclosed curly brace in generated RHP code");
    assert!(!in_string, "Unclosed string literal in generated RHP code");

    // Verify all statement lines end with ';' or '{' or '}'
    for line in rhp_code.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_valid_line = trimmed.ends_with(';')
            || trimmed.ends_with('{')
            || trimmed.ends_with('}')
            || trimmed.starts_with("//");
        assert!(
            is_valid_line,
            "Line in generated RHP code does not conform to syntax (must end in ';', '{{', or '}}'): '{}'",
            trimmed
        );
    }
}

#[test]
fn test_complex_template_e2e_compilation() {
    let template = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>{{ $page.title }} - My Store</title>
    {{-- Internal template note: tracking script below --}}
    <meta name="description" content="{!! $page.meta_description !!}">
</head>
<body>
    <header class="header">
        <h1>Welcome to {{ $site.name }}</h1>
        @datetime($page.created_at)
        @badge
    </header>

    @rhp
    let cart_count = $cart.items.len();
    let is_logged_in = $user.is_authenticated;
    @endrhp

    @if($is_logged_in && $user.is_vip)
        <div class="vip-banner">
            <span>VIP Member: {{ $user.username }}</span>
            <span>Badge: {!! $user.vip_html !!}</span>
        </div>
    @elseif($is_logged_in)
        <div class="user-greeting">
            <span>Welcome back, {{ $user.username }}!</span>
        </div>
    @else
        <div class="guest-banner">
            <span>Please <a href="/login">sign in</a></span>
        </div>
    @endif

    <section class="cart-section">
        @if($cart_count > 0)
            <h2>Your Cart ({{ $cart_count }} items)</h2>
            <ul class="cart-list">
            @for($i = 0; $i < $cart_count; $i++)
                <li>
                    Item #{{ $i + 1 }}: {{ $cart.items[$i].title }}
                    @if($cart.items[$i].is_on_sale)
                        <span class="sale-badge">SALE!</span>
                    @endif
                </li>
            @endfor
            </ul>
        @else
            <p>Your shopping cart is currently empty.</p>
        @endif
    </section>

    <footer id="app">
        <p>Copyright &copy; 2026</p>
        <div class="vue-container">
            <span>Vue Bound: @{{ appState.activeUser }}</span>
        </div>
    </footer>
</body>
</html>"#;

    let mut compiler = Compiler::standalone();
    compiler
        .register_directive("datetime", |args| {
            format!("$__out += Date::format({}, \"Y-m-d\");", args.unwrap_or("''"))
        })
        .register_directive("badge", |_args| {
            "$__out += \"<span class=\\\"badge\\\">Official</span>\";".to_string()
        });

    let compiled = compiler.compile(template).expect("Compilation must succeed");

    // 1. Verify RHP syntax conformance
    verify_rhp_syntax(&compiled, OutputFormat::Standalone);

    // 2. Verify expected structural statements
    assert!(compiled.starts_with("$__out = \"\";"));
    assert!(compiled.ends_with("return $__out;"));
    assert!(compiled.contains("$__out += Html::escape($page.title);"));
    assert!(compiled.contains("$__out += string($page.meta_description);"));
    assert!(compiled.contains("$__out += Date::format($page.created_at, \"Y-m-d\");"));
    assert!(compiled.contains("$__out += \"<span class=\\\"badge\\\">Official</span>\";"));
    assert!(compiled.contains("let cart_count = $cart.items.len();"));
    assert!(compiled.contains("let is_logged_in = $user.is_authenticated;"));
    assert!(compiled.contains("if ($is_logged_in && $user.is_vip) {"));
    assert!(compiled.contains("} else if ($is_logged_in) {"));
    assert!(compiled.contains("} else {"));
    assert!(compiled.contains("for ($i = 0; $i < $cart_count; $i++) {"));
    assert!(compiled.contains("$__out += \"{{ appState.activeUser }}\";"));

    // 3. Verify comments stripped
    assert!(!compiled.contains("Internal template note"));

    // 4. Test closure output format with the same template
    compiler.set_format(OutputFormat::Closure);
    let compiled_closure = compiler.compile(template).expect("Closure compilation must succeed");
    verify_rhp_syntax(&compiled_closure, OutputFormat::Closure);
}

#[test]
fn test_view_cache_development_mode_mtime_recompile() {
    let test_dir = create_test_dir("view_cache_dev");
    let views_dir = test_dir.join("views");
    let storage_dir = test_dir.join("storage");
    fs::create_dir_all(&views_dir).unwrap();
    fs::create_dir_all(&storage_dir).unwrap();

    let template_file = views_dir.join("profile.blade.rhp");
    fs::write(
        &template_file,
        "<h1>Hello {{ $name }}</h1>\n<p>Version 1</p>",
    )
    .unwrap();

    let cache = ViewCache::dev(&storage_dir);
    assert!(cache.mode().is_development());

    // First load in dev mode: cache file does not exist, so it must compile
    let cached_path = cache.get_cache_path(&template_file);
    assert!(!cached_path.exists());
    assert!(cache.is_expired(&template_file).unwrap());

    let output_v1 = cache.load(&template_file).unwrap();
    assert!(cached_path.exists());
    assert!(output_v1.contains("Version 1"));
    verify_rhp_syntax(&output_v1, OutputFormat::Standalone);

    let cache_meta_v1 = fs::metadata(&cached_path).unwrap();
    let cache_mtime_v1 = cache_meta_v1.modified().unwrap();

    // Small delay to ensure any subsequent filesystem changes get a distinct mtime
    thread::sleep(Duration::from_millis(50));

    // Second load without modifying source: is_expired must be false, mtime unchanged
    assert!(!cache.is_expired(&template_file).unwrap());
    let output_v2 = cache.load(&template_file).unwrap();
    assert_eq!(output_v1, output_v2);

    let cache_meta_v2 = fs::metadata(&cached_path).unwrap();
    assert_eq!(
        cache_mtime_v1,
        cache_meta_v2.modified().unwrap(),
        "Cache file must not be modified or recompiled when source template has not changed"
    );

    // Modify the source template file (Version 2)
    thread::sleep(Duration::from_millis(50));
    fs::write(
        &template_file,
        "<h1>Hello {{ $name }}</h1>\n<p>Version 2 Updated</p>",
    )
    .unwrap();

    // Now source template mtime > cached file mtime; is_expired must return true
    assert!(
        cache.is_expired(&template_file).unwrap(),
        "is_expired must return true when source mtime > cached mtime"
    );

    // Load in dev mode must recompile and return updated version
    let output_v3 = cache.load(&template_file).unwrap();
    assert!(output_v3.contains("Version 2 Updated"));

    let cache_meta_v3 = fs::metadata(&cached_path).unwrap();
    assert!(
        cache_meta_v3.modified().unwrap() > cache_mtime_v1,
        "Cache file must have been recompiled and updated"
    );
}

#[test]
fn test_view_cache_production_mode_direct_load() {
    let test_dir = create_test_dir("view_cache_prod");
    let views_dir = test_dir.join("views");
    let storage_dir = test_dir.join("storage");
    fs::create_dir_all(&views_dir).unwrap();
    fs::create_dir_all(&storage_dir).unwrap();

    let template_file = views_dir.join("index.blade.rhp");
    fs::write(&template_file, "<div>Original Content</div>").unwrap();

    let mut cache = ViewCache::dev(&storage_dir);
    // Pre-warm the cache
    let cached_path = cache.compile(&template_file).unwrap();
    assert!(cached_path.exists());

    // Switch to Production mode
    cache.set_mode(ViewMode::Production);
    assert!(cache.mode().is_production());

    // Modify the source template file on disk
    thread::sleep(Duration::from_millis(50));
    fs::write(&template_file, "<div>Modified Content After Deploy</div>").unwrap();

    // In production mode: MUST load cached compiled .rhp directly without checking template mtime!
    let prod_output = cache.load(&template_file).unwrap();
    assert!(
        prod_output.contains("Original Content"),
        "Production mode must load the cached compiled .rhp directly without checking source template mtime"
    );
    assert!(!prod_output.contains("Modified Content After Deploy"));

    // Even if source template is completely deleted, production mode loads cached file directly!
    fs::remove_file(&template_file).unwrap();
    let prod_output_after_source_deleted = cache.load(&template_file).unwrap();
    assert!(prod_output_after_source_deleted.contains("Original Content"));

    // If cache file is missing in production mode, it returns CacheNotFound error
    let missing_template = views_dir.join("nonexistent.blade.rhp");
    let err = cache.load(&missing_template).unwrap_err();
    match err {
        ViewError::CacheNotFound { .. } => {}
        other => panic!("Expected ViewError::CacheNotFound, got: {other:?}"),
    }
}

#[test]
fn test_view_engine_view_resolution_and_dot_syntax() {
    let test_dir = create_test_dir("view_engine");
    let views_dir = test_dir.join("views");
    let storage_dir = test_dir.join("storage");
    let auth_dir = views_dir.join("auth");
    let admin_settings_dir = views_dir.join("admin").join("settings");

    fs::create_dir_all(&auth_dir).unwrap();
    fs::create_dir_all(&admin_settings_dir).unwrap();
    fs::create_dir_all(&storage_dir).unwrap();

    fs::write(
        views_dir.join("home.blade.rhp"),
        "<h1>Home: {{ $title }}</h1>",
    )
    .unwrap();
    fs::write(
        auth_dir.join("login.blade.rhp"),
        "<form>Login: {{ $user }}</form>",
    )
    .unwrap();
    fs::write(
        admin_settings_dir.join("security.blade.rhp"),
        "<div>Security Settings</div>",
    )
    .unwrap();

    let mut engine = ViewEngine::dev(&views_dir, &storage_dir);
    engine.register_directive("uppercase", |args| {
        format!("$__out += Str::upperCase({});", args.unwrap_or("''"))
    });

    // Test resolving direct view name
    let home_path = engine.resolve_view_path("home").unwrap();
    assert!(home_path.ends_with("views/home.blade.rhp"));

    // Test resolving dot notation (auth.login)
    let login_path = engine.resolve_view_path("auth.login").unwrap();
    assert!(login_path.ends_with("views/auth/login.blade.rhp"));

    // Test resolving deep dot notation (admin.settings.security)
    let sec_path = engine.resolve_view_path("admin.settings.security").unwrap();
    assert!(sec_path.ends_with("views/admin/settings/security.blade.rhp"));

    // Test loading and compiling views
    let home_compiled = engine.load("home").unwrap();
    assert!(home_compiled.contains("Home: "));
    verify_rhp_syntax(&home_compiled, OutputFormat::Standalone);

    let login_compiled = engine.load("auth.login").unwrap();
    assert!(login_compiled.contains("Login: "));
    verify_rhp_syntax(&login_compiled, OutputFormat::Standalone);

    // Test cache_all on engine
    let count = engine.cache_all().unwrap();
    assert_eq!(count, 3, "Must recursively compile all 3 .blade.rhp views");

    // Test clear on engine
    let cleared = engine.clear().unwrap();
    assert_eq!(cleared, 3, "Must clear all 3 cached .rhp files");
}

#[test]
fn test_cli_compile_stdout() {
    let test_dir = create_test_dir("cli_compile_stdout");
    let template_file = test_dir.join("sample.blade.rhp");
    fs::write(
        &template_file,
        "<div class=\"container\">\n    <h1>{{ $title }}</h1>\n    {!! $body !!}\n</div>",
    )
    .unwrap();

    let bin_path = env!("CARGO_BIN_EXE_rade");
    let output = Command::new(bin_path)
        .arg("compile")
        .arg(&template_file)
        .output()
        .expect("Failed to execute rade compile");

    assert!(output.status.success(), "rade compile must exit with 0");
    let stdout = String::from_utf8(output.stdout).unwrap();

    verify_rhp_syntax(&stdout, OutputFormat::Standalone);
    assert!(stdout.contains("$__out += Html::escape($title);"));
    assert!(stdout.contains("$__out += string($body);"));
}

#[test]
fn test_cli_compile_to_file() {
    let test_dir = create_test_dir("cli_compile_file");
    let template_file = test_dir.join("input.blade.rhp");
    let output_file = test_dir.join("output").join("compiled.rhp");

    fs::write(
        &template_file,
        "@if($show)<span>Visible: {{ $msg }}</span>@endif",
    )
    .unwrap();

    let bin_path = env!("CARGO_BIN_EXE_rade");
    let output = Command::new(bin_path)
        .arg("compile")
        .arg(&template_file)
        .arg("-o")
        .arg(&output_file)
        .output()
        .expect("Failed to execute rade compile with -o");

    assert!(output.status.success());
    assert!(output_file.exists(), "Output file must be written to disk");

    let file_content = fs::read_to_string(&output_file).unwrap();
    verify_rhp_syntax(&file_content, OutputFormat::Standalone);
    assert!(file_content.contains("if ($show) {"));
    assert!(file_content.contains("$__out += Html::escape($msg);"));
}

#[test]
fn test_cli_cache_and_clear_workflow() {
    let test_dir = create_test_dir("cli_cache_clear");
    let views_dir = test_dir.join("views");
    let storage_dir = test_dir.join("storage");
    let sub_dir = views_dir.join("partials");

    fs::create_dir_all(&sub_dir).unwrap();
    fs::create_dir_all(&storage_dir).unwrap();

    // Create multiple .blade.rhp files
    fs::write(views_dir.join("app.blade.rhp"), "<html>{{ $content }}</html>").unwrap();
    fs::write(views_dir.join("nav.blade.rhp"), "<nav>{{ $user }}</nav>").unwrap();
    fs::write(sub_dir.join("footer.blade.rhp"), "<footer>&copy; 2026</footer>").unwrap();

    // Create a non-blade file that should be ignored
    fs::write(views_dir.join("notes.txt"), "This is not a template").unwrap();

    // Create a non-rhp file in storage that should NOT be deleted by clear
    fs::write(storage_dir.join(".gitkeep"), "").unwrap();

    let bin_path = env!("CARGO_BIN_EXE_rade");

    // 1. Run: rade cache <views_dir> <storage_dir>
    let cache_output = Command::new(bin_path)
        .arg("cache")
        .arg(&views_dir)
        .arg(&storage_dir)
        .output()
        .expect("Failed to run rade cache");

    assert!(cache_output.status.success());
    let stdout = String::from_utf8(cache_output.stdout).unwrap();
    assert!(stdout.contains("Cached 3 view(s)"));

    // Verify exactly 3 .rhp cache files were generated with sha1 names
    let mut rhp_count = 0;
    for entry in fs::read_dir(&storage_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) == Some("rhp") {
            rhp_count += 1;
            let filename = path.file_stem().unwrap().to_str().unwrap();
            assert_eq!(
                filename.len(),
                40,
                "Cache file stem must be a 40-character SHA-1 hex string"
            );
            let content = fs::read_to_string(&path).unwrap();
            verify_rhp_syntax(&content, OutputFormat::Standalone);
        }
    }
    assert_eq!(rhp_count, 3);
    assert!(storage_dir.join(".gitkeep").exists());

    // 2. Run: rade clear <storage_dir>
    let clear_output = Command::new(bin_path)
        .arg("clear")
        .arg(&storage_dir)
        .output()
        .expect("Failed to run rade clear");

    assert!(clear_output.status.success());
    let clear_stdout = String::from_utf8(clear_output.stdout).unwrap();
    assert!(clear_stdout.contains("Cleared 3 cached view file(s)"));

    // Verify all .rhp files were deleted, but .gitkeep remains
    let remaining_files: Vec<_> = fs::read_dir(&storage_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(remaining_files, vec![".gitkeep"]);
}

#[test]
fn test_cli_error_handling() {
    let bin_path = env!("CARGO_BIN_EXE_rade");

    // Missing input file for compile
    let out = Command::new(bin_path).arg("compile").output().unwrap();
    assert!(!out.status.success());

    // Nonexistent input file for compile
    let out = Command::new(bin_path)
        .arg("compile")
        .arg("nonexistent_path_to_template.blade.rhp")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Error reading input file"));

    // Missing args for cache
    let out = Command::new(bin_path).arg("cache").output().unwrap();
    assert!(!out.status.success());

    // Missing args for clear
    let out = Command::new(bin_path).arg("clear").output().unwrap();
    assert!(!out.status.success());

    // Unknown command
    let out = Command::new(bin_path).arg("foobar").output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Unknown command 'foobar'"));
}

#[test]
fn test_sha1_implementation_standard_vectors() {
    // RFC 3174 test vectors
    assert_eq!(sha1(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(sha1(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(
        sha1_str("The quick brown fox jumps over the lazy dog"),
        "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
    );
}

#[test]
fn test_cli_cache_into_production_engine_workflow() {
    let test_dir = create_test_dir("cli_cache_into_prod");
    let views_dir = test_dir.join("views");
    let storage_dir = test_dir.join("storage");
    let reports_dir = views_dir.join("reports");

    fs::create_dir_all(&reports_dir).unwrap();
    fs::create_dir_all(&storage_dir).unwrap();

    fs::write(
        views_dir.join("welcome.blade.rhp"),
        "<h1>Hello {{ $user }}</h1>\n<p>Welcome to Rade!</p>",
    )
    .unwrap();
    fs::write(
        reports_dir.join("monthly.blade.rhp"),
        "<h2>Monthly Report for {{ $month }}</h2>\n@if($approved)<span>Approved</span>@endif",
    )
    .unwrap();

    let bin_path = env!("CARGO_BIN_EXE_rade");

    // Pre-compile via CLI: rade cache <views_dir> <storage_dir>
    let status = Command::new(bin_path)
        .arg("cache")
        .arg(&views_dir)
        .arg(&storage_dir)
        .status()
        .expect("Failed to execute rade cache");
    assert!(status.success());

    // Verify cache files exist
    let entries: Vec<_> = fs::read_dir(&storage_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("rhp"))
        .collect();
    assert_eq!(entries.len(), 2);

    // Initialize ViewEngine in Production mode
    let engine = ViewEngine::prod(&views_dir, &storage_dir);
    assert!(engine.mode().is_production());

    // Load views in production mode directly from cache
    let welcome_out = engine.load("welcome").unwrap();
    assert!(welcome_out.contains("Welcome to Rade!"));
    verify_rhp_syntax(&welcome_out, OutputFormat::Standalone);

    let monthly_out = engine.load("reports.monthly").unwrap();
    assert!(monthly_out.contains("Monthly Report for"));
    assert!(monthly_out.contains("if ($approved) {"));
    verify_rhp_syntax(&monthly_out, OutputFormat::Standalone);

    // In production mode, modifying the source file has NO effect on output
    fs::write(
        views_dir.join("welcome.blade.rhp"),
        "<h1>Modified after build</h1>",
    )
    .unwrap();

    let welcome_out_after_mod = engine.load("welcome").unwrap();
    assert_eq!(
        welcome_out, welcome_out_after_mod,
        "Production engine must load cached output and ignore modifications to source file"
    );
}

#[test]
fn test_cli_compile_flag_ordering_and_formats() {
    let test_dir = create_test_dir("cli_flags");
    let in_file = test_dir.join("card.blade.rhp");
    let out_file1 = test_dir.join("out1.rhp");
    let out_file2 = test_dir.join("out2.rhp");

    fs::write(
        &in_file,
        "<div class=\"card\">{{ $content }}</div>",
    )
    .unwrap();

    let bin_path = env!("CARGO_BIN_EXE_rade");

    // Flag order: rade compile -o <out> <in>
    let status1 = Command::new(bin_path)
        .arg("compile")
        .arg("-o")
        .arg(&out_file1)
        .arg(&in_file)
        .status()
        .unwrap();
    assert!(status1.success());
    assert!(out_file1.exists());
    let c1 = fs::read_to_string(&out_file1).unwrap();
    verify_rhp_syntax(&c1, OutputFormat::Standalone);

    // Flag format: rade compile --output=<out> <in>
    let status2 = Command::new(bin_path)
        .arg("compile")
        .arg(format!("--output={}", out_file2.display()))
        .arg(&in_file)
        .status()
        .unwrap();
    assert!(status2.success());
    assert!(out_file2.exists());
    let c2 = fs::read_to_string(&out_file2).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn test_cache_key_path_consistency() {
    let test_dir = create_test_dir("cache_key_consistency");
    let file = test_dir.join("test.blade.rhp");
    fs::write(&file, "<span>test</span>").unwrap();

    // Canonical / absolute path vs relative path
    let abs_key = rade::runtime::cache_key_for_path(&file);

    // Relative path to the same file
    let current_dir = std::env::current_dir().unwrap();
    if let Ok(rel_path) = file.strip_prefix(&current_dir) {
        let rel_key = rade::runtime::cache_key_for_path(rel_path);
        assert_eq!(abs_key, rel_key, "Canonicalization must yield identical cache keys for absolute and relative paths");
    }
}
