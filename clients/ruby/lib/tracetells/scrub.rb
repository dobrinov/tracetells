# frozen_string_literal: true

module Tracetells
  # The same scrubbing Trace's server does (crates/trace-core/src/privacy.rs
  # and errors.rs), done here first so nothing personal leaves the server.
  # clients/test-vectors.json holds the cases every client must agree on.
  module Scrub
    EMAIL = /[a-z0-9._%+-]+(@|%40)[a-z0-9.-]+\.[a-z]{2,}/i
    JWT = /\beyJ[\w-]+\.[\w-]+\.[\w-]+/
    URL_QUERY = %r{(https?://[^\s?#]+)[?#][^\s]*}
    IBAN = /\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b/
    CARD = /\b(?:\d[ -]?){12,18}\d\b/
    TOKEN = %r{[A-Za-z0-9_\-+/=]{20,}}
    LONG_NUMBER = /\+?\d(?:[\s().-]{0,2}\d){6,}/
    HIDDEN = /[\u0000-\u001f\u007f-\u009f\u00ad\u200b-\u200f\u2028-\u202e\u2060-\u2069\ufeff]/

    DEPENDENCY_ROOTS = ["node_modules/", "gems/", "registry/src/", "site-packages/", "vendor/"].freeze
    PROJECT_ROOTS = ["src/", "app/", "lib/", "crates/", "packages/", "config/", "bin/", "spec/", "test/", "tests/"].freeze

    module_function

    def token?(s)
      s.match?(/\d/) && s.match?(/[A-Za-z]/)
    end

    # On one line: control and invisible characters become spaces, backticks quotes.
    def one_line(s)
      s.to_s.scrub("?").gsub(HIDDEN, " ").tr("`", "'").squeeze(" ").strip
    end

    def truncate(s, max)
      s.length > max ? s[0, max] : s
    end

    # Emails, tokens, card numbers, IBANs, phone-like numbers and URL query strings.
    def message(s)
      one_line(s)
        .gsub(URL_QUERY, '\1?[redacted]')
        .gsub(EMAIL, "[email]")
        .gsub(JWT, "[token]")
        .gsub(IBAN, "[iban]")
        .gsub(CARD, "[card]")
        .gsub(TOKEN) { |m| token?(m) ? "[token]" : m }
        .gsub(LONG_NUMBER, "[number]")
    end

    # A route as Trace stores it: no query string, segments that look like ids,
    # emails or tokens replaced. Route parameters (:id, {id}) pass through.
    def route(path)
      p = one_line(path.to_s.strip).gsub(" ", "%20").split(/[?#]/).first.to_s
      return "" unless p.start_with?("/")

      out = p.split("/", -1).map do |seg|
        if seg.length > 2 && seg.start_with?("{") && seg.end_with?("}")
          ":#{seg[1..-2].delete_prefix('*')}"
        elsif seg.empty? || seg.start_with?(":") || seg == "*"
          seg
        elsif seg.include?("@") || seg.downcase.include?("%40")
          ":email"
        elsif seg.include?("=") || (seg.count(".") >= 2 && seg.length >= 24)
          ":token"
        elsif seg.count("0-9") >= 3 || (seg.length >= 16 && token?(seg)) || (seg.length >= 16 && seg.match?(/\A\h+\z/))
          ":id"
        else
          seg
        end
      end.join("/")
      truncate(out, 200)
    end

    # A frame's file, relative to the project or the dependency it is in.
    def file(name)
      f = one_line(name).tr("\\", "/")
      f = f.delete_prefix("file://")
      f = f.split(/[?#]/).first.to_s
      absolute = f.start_with?("/") || f[1] == ":" || f.start_with?("~/")
      rel =
        if !absolute
          f
        elsif (dep = DEPENDENCY_ROOTS.filter_map { |m| (i = f.rindex(m)) && [i, m] }.max_by(&:first))
          i, m = dep
          if m == "registry/src/"
            rest = f[(i + m.length)..]
            (slash = rest.index("/")) ? rest[(slash + 1)..] : rest
          else
            f[i..]
          end
        elsif (i = PROJECT_ROOTS.lazy.map { |m| f.index("/#{m}") }.find(&:itself))
          f[(i + 1)..]
        else
          f.split("/").last.to_s
        end
      truncate(rel.split("/", -1).map { |seg| seg.include?("@") ? ":email" : seg }.join("/"), 200)
    end
  end
end
