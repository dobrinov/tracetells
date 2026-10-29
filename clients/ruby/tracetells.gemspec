require_relative "lib/tracetells/version"

Gem::Specification.new do |s|
  s.name = "tracetells"
  s.version = Tracetells::VERSION
  s.summary = "Report Ruby server errors to Trace (tracetells.com): error facts only, scrubbed, in the background"
  s.description = "Sends the class, message, backtrace, route, status and release of your app's errors to Trace, which groups and counts them and ties them to the visits they broke. Never request bodies, params, headers, cookies or users."
  s.authors = ["Deyan Dobrinov"]
  s.email = "hello@tracetells.com"
  s.homepage = "https://tracetells.com/docs/server-errors/"
  s.license = "MIT"
  s.required_ruby_version = ">= 3.1"
  s.files = Dir["lib/**/*.rb", "README.md", "LICENSE"]
  s.metadata = { "source_code_uri" => "https://github.com/dobrinov/tracetells/tree/main/clients/ruby", "rubygems_mfa_required" => "true" }
end
