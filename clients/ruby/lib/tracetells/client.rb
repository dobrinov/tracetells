# frozen_string_literal: true

module Tracetells
  # Collects errors and sends them from a background thread: in batches, at
  # most once a second, with repeats within a minute folded into a count, and
  # backing off when Trace is unreachable or asks to slow down.
  class Client
    QUEUE = 1000
    BATCH = 100
    FOLD = 60
    SEND_EVERY = 1.0

    attr_reader :environment, :release

    def initialize(key:, endpoint:, environment:, release:)
      @key = key
      @uri = URI("#{endpoint}/v1/errors")
      @environment = environment.to_s
      @release = release.to_s
      @queue = Thread::Queue.new
      @recent = {}
      @outgoing = []
      @paused_until = 0
      @backoff = 5
      @lock = Mutex.new
      @pid = Process.pid
    end

    def report(error, request)
      enqueue(event(error.class.name || "Error", Scrub.message(chain(error)), error.backtrace || caller(2), request))
    end

    def report_message(class_name, message, request)
      enqueue(event(class_name.to_s, Scrub.message(message), caller(2), request))
    end

    def flush(timeout)
      done = Thread::Queue.new
      start
      @queue << [:flush, done]
      done.pop(timeout: timeout)
    end

    def shutdown
      @queue << [:stop, nil]
    end

    private

    def chain(error)
      msg = error.message.to_s
      cause = error.cause
      3.times do
        break unless cause

        msg += ": #{cause.message}"
        cause = cause.cause
      end
      msg
    end

    def event(class_name, message, backtrace, request)
      request ||= {}
      status = request[:status].to_i
      {
        "class" => Scrub.truncate(Scrub.one_line(class_name), 200),
        "message" => Scrub.truncate(message, 300),
        "backtrace" => frames(backtrace),
        "route" => request[:route].to_s.empty? ? "" : Scrub.route(request[:route]),
        "method" => request[:method].to_s.upcase,
        "status" => status.between?(100, 599) ? status : nil,
        "occurred_at" => (Time.now.to_f * 1000).to_i,
        "count" => 1
      }
    end

    RUBY_LIB = RbConfig::CONFIG["rubylibdir"].to_s

    # "/srv/app/app/models/order.rb:12:in 'Order#total'" (Ruby 3.4+ quotes, older ones backtick-quote).
    def frames(lines)
      Array(lines).first(60).filter_map do |line|
        m = /\A(.+?):(\d+)(?::in [`'](.+)')?\z/.match(line.to_s)
        next unless m

        file = m[1]
        next if file.include?("/lib/tracetells")

        dependency = file.include?("/gems/") || file.start_with?("<internal:") || (!RUBY_LIB.empty? && file.start_with?(RUBY_LIB))
        { "file" => Scrub.file(file), "line" => m[2].to_i, "function" => Scrub.truncate(Scrub.one_line(m[3].to_s), 200), "in_app" => !dependency }
      end.first(40)
    end

    def enqueue(e)
      start
      @queue << [:event, e] if @queue.size < QUEUE
    end

    # After a fork (Puma, Unicorn) the thread is gone: start a new one.
    def start
      @lock.synchronize do
        if @pid != Process.pid
          @pid = Process.pid
          @thread = nil
        end
        @thread = Thread.new { run } unless @thread&.alive?
      end
    end

    def run
      Thread.current.name = "tracetells"
      Thread.current.report_on_exception = false
      last_send = monotonic
      loop do
        kind, payload = @queue.pop(timeout: SEND_EVERY)
        flushing = nil
        case kind
        when :event then admit(payload)
        when :flush then flushing = payload
        when :stop then return
        end
        fold_expired(!flushing.nil?)
        if !@outgoing.empty? && (flushing || @outgoing.size >= BATCH || monotonic - last_send >= SEND_EVERY)
          send_all
          last_send = monotonic
        end
        flushing&.push(true)
      end
    rescue StandardError
      nil
    end

    def admit(e)
      key = fold_key(e)
      seen = @recent[key]
      if seen && monotonic - seen[:start] < FOLD
        seen[:repeats] += 1
        seen[:latest] = e
      else
        @recent[key] = { start: monotonic, repeats: 0, latest: nil }
        @outgoing << e
      end
    end

    def fold_key(e)
      top = e["backtrace"].find { |f| f["in_app"] }
      [e["class"], top ? "#{top['file']}|#{top['function']}" : e["message"], e["route"], e["status"]].join("|")
    end

    # Minutes that ended: their repeats go out as one error with a count.
    def fold_expired(all)
      now = monotonic
      @recent.delete_if do |_, r|
        ended = now - r[:start] >= FOLD
        if (ended || all) && r[:latest] && r[:repeats].positive?
          @outgoing << r[:latest].merge("count" => r[:repeats])
          r[:repeats] = 0
          r[:latest] = nil
        end
        ended
      end
    end

    def send_all
      until @outgoing.empty?
        batch = @outgoing.shift(BATCH)
        next if monotonic < @paused_until # backing off: drop rather than pile up

        runtime = { "name" => RUBY_ENGINE, "version" => RUBY_VERSION }
        body = JSON.generate("errors" => batch.map { |e| e.merge("environment" => @environment, "release" => @release, "runtime" => runtime) })
        begin
          res = Net::HTTP.start(@uri.host, @uri.port, use_ssl: @uri.scheme == "https", open_timeout: 5, read_timeout: 5, write_timeout: 5) do |http|
            http.post(@uri.path, body, "authorization" => "Bearer #{@key}", "content-type" => "application/json")
          end
          code = res.code.to_i
          raise "HTTP #{code}" if code == 429 || code >= 500

          @backoff = 5
        rescue StandardError
          @paused_until = monotonic + @backoff
          @backoff = [@backoff * 2, 300].min
        end
      end
    end

    def monotonic
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end
  end
end
