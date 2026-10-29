# frozen_string_literal: true

require "helper"
require "tracetells"

class ScrubTest < Minitest::Test
  VECTORS = JSON.parse(File.read(File.expand_path("../../test-vectors.json", __dir__)))

  def test_matches_the_shared_vectors
    VECTORS["message"].each { |input, want| assert_equal want, Tracetells::Scrub.message(input), "message #{input}" }
    VECTORS["route"].each { |input, want| assert_equal want, Tracetells::Scrub.route(input), "route #{input}" }
    VECTORS["file"].each { |input, want| assert_equal want, Tracetells::Scrub.file(input), "file #{input}" }
    assert_equal "/orders/:id", Tracetells::Scrub.route("/orders/{id}")
  end
end
