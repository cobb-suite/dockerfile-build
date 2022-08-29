mod utils;
use std::fmt;

use bollard::{image::BuildImageOptions, service::BuildInfo, Docker};
use futures::stream::StreamExt;
use thiserror::Error;
use tracing::{event, Level};

use crate::utils::tarball;

#[derive(Error, Debug)]
pub(crate) enum DockerfileError<'a> {
    #[error("Building image via Docker API failed: DockerfileImage: {dockerfile_image}")]
    BuildImage {
        error: String,
        dockerfile_image: &'a DockerfileImage,
    },
}

#[derive(Debug)]
pub(crate) struct DockerfileImage {
    repository: String,
    tag: String,
    path: String,
    name: String,
    #[cfg(feature = "dockertest")]
    image: Dockertest::Image
}

impl fmt::Display for DockerfileImage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "repository: {}, tag: {}, path: {}, name: {}",
            self.repository, self.tag, self.path, self.name
        )
    }
}

impl DockerfileImage {
    pub fn with_dockerfile<T: ToString>(
        repository: T,
        tag: Option<T>,
        path: Option<T>,
        name: Option<T>,
    ) -> DockerfileImage {
        DockerfileImage {
            repository: repository.to_string(),
            tag: tag.map_or("latest".to_string(), |tag| tag.to_string()),
            path: path.map_or("./dockerfile".to_string(), |path| path.to_string()),
            name: name.map_or("Dockerfile".to_string(), |name| name.to_string()),
            #[cfg(feature = "dockertest")]
            image: Dockertest::Image::with_repository(repository)
        }
    }

    #[cfg(feature = "dockertest")]
    pub(crate) fn image(&self) -> &Dockertest::Image {
        &self.image
    }

    pub async fn build(&self, client: &Docker) -> Result<(), DockerfileError> {
        dbg!("building image: {}:{}", &self.repository, &self.tag);
        let options = BuildImageOptions::<&str> {
            dockerfile: &self.name,
            t: &format!("{}:{}", &self.repository, &self.tag), // This is the tag we would give the image when building, docker build . -t <name:tag>
            rm: true,
            ..Default::default()
        };

        let buf = tarball(&self.path, &self.name).unwrap();

        let mut stream = client.build_image(options, None, Some(buf.into()));
        while let Some(result) = stream.next().await {
            match result {
                Ok(intermitten_result) => match intermitten_result {
                    BuildInfo {
                        id,
                        stream: _,
                        error,
                        error_detail,
                        status,
                        progress,
                        progress_detail,
                        aux: _,
                    } => {
                        if error.is_some() {
                            event!(
                                Level::ERROR,
                                "build error {} {:?}",
                                error.clone().unwrap(),
                                error_detail.clone().unwrap()
                            );
                        } else {
                            event!(
                                Level::TRACE,
                                "build progress {} {:?} {:?} {:?}",
                                status.clone().unwrap_or_default(),
                                id.clone().unwrap_or_default(),
                                progress.clone().unwrap_or_default(),
                                progress_detail.clone().unwrap_or_default(),
                            );
                        }
                    }
                },
                Err(e) => {
                    let msg = e.to_string();
                    return Err(DockerfileError::BuildImage {
                        error: msg,
                        dockerfile_image: self,
                    });
                }
            }
        }

        event!(Level::DEBUG, "successfully built image");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bollard::Docker;
    use wiremock::{matchers::method, matchers::path, Mock, MockServer, ResponseTemplate};

    use crate::DockerfileImage;

    #[tokio::test]
    async fn it_works() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/build"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1..)
            .mount(&mock_server)
            .await;
        let client =
            Docker::connect_with_http(&mock_server.uri(), 4, bollard::API_DEFAULT_VERSION).unwrap();

        let img = DockerfileImage::with_dockerfile(
            "dockertest-dockerfile/hello",
            None,
            Some("./dockerfiles/hello.dockerfile"),
            None,
        );
        img.build(&client).await.unwrap();

        let received_requests = mock_server.received_requests().await.unwrap();
        let request = received_requests.get(0).unwrap();
        dbg!(request);
        let mut params = request.url.query_pairs();
        assert!(
            params.any(|x| x.0.eq("t")
                && x.1.contains("dockertest-dockerfile")
                && x.1.contains("hello")
                && x.1.contains("latest")),
            "failure checking build image request contains tag"
        );
    }

    #[tokio::test]
    async fn it_works_custom_tag() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/build"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1..)
            .mount(&mock_server)
            .await;
        let client =
            Docker::connect_with_http(&mock_server.uri(), 4, bollard::API_DEFAULT_VERSION).unwrap();

        let img = DockerfileImage::with_dockerfile(
            "dockertest-dockerfile/hello",
            Some("stable"),
            Some("./dockerfiles/hello.dockerfile"),
            None,
        );
        img.build(&client).await.unwrap();

        let received_requests = mock_server.received_requests().await.unwrap();
        let request = received_requests.get(0).unwrap();
        dbg!(request);
        let mut params = request.url.query_pairs();
        assert!(
            params.any(|x| x.0.eq("t")
                && x.1.contains("dockertest-dockerfile")
                && x.1.contains("hello")
                && x.1.contains("stable")),
            "failure checking build image request contains tag"
        );
    }
}
