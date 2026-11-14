mp.register_event("end-file", function(event)
    if event.reason == "eof" then
        mp.command("playlist-remove current")
    end
end)
