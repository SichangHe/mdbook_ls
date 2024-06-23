(function() {
    const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
    const wsAddress =
        `${wsProtocol}://${location.host}/__mdbook_incremental_preview_live_patch${location.pathname}`;
    const web_socket = new WebSocket(wsAddress);
    web_socket.onclose = () => location.reload();
    // NOTE: We assume that the content is in <main> as per `index.hbs`.
    const contentElement = document.querySelector("main");
    web_socket.onmessage = (event) => {
        contentElement.innerHTML = event.data;
        document.dispatchEvent(new Event("load"));
    };
})();
