# frozen_string_literal: true

require "minitest/autorun"
require "socket"
require "json"

# A stand-in for Trace: keeps every batch it receives.
class FakeTrace
  attr_reader :received, :port

  def initialize
    @received = []
    @server = TCPServer.new("127.0.0.1", 0)
    @port = @server.addr[1]
    @thread = Thread.new { loop { handle(@server.accept) } }
  end

  def errors
    @received.flat_map { |r| r[:body]["errors"] }
  end

  def close
    @thread.kill
    @server.close
  end

  private

  def handle(socket)
    head = +""
    head << socket.gets.to_s until head.end_with?("\r\n\r\n")
    length = head[/content-length: (\d+)/i, 1].to_i
    body = socket.read(length)
    @received << { auth: head[/authorization: (.+?)\r\n/i, 1], body: JSON.parse(body) }
    socket.write("HTTP/1.1 202 Accepted\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
  ensure
    socket.close
  end
end
