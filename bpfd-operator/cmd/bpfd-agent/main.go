/*
Copyright 2022.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

package main

import (
	"context"
	"flag"
	"fmt"
	"os"

	// Import all Kubernetes client auth plugins (e.g. Azure, GCP, OIDC, etc.)
	// to ensure that exec-entrypoint and run can make use of them.
	_ "k8s.io/client-go/plugin/pkg/client/auth"

	"k8s.io/apimachinery/pkg/runtime"
	utilruntime "k8s.io/apimachinery/pkg/util/runtime"
	clientgoscheme "k8s.io/client-go/kubernetes/scheme"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/healthz"
	"sigs.k8s.io/controller-runtime/pkg/log/zap"

	bpfdiov1alpha1 "github.com/bpfd-dev/bpfd/bpfd-operator/apis/v1alpha1"
	bpfdagent "github.com/bpfd-dev/bpfd/bpfd-operator/controllers/bpfd-agent"

	"github.com/bpfd-dev/bpfd/bpfd-operator/internal/tls"
	gobpfd "github.com/bpfd-dev/bpfd/clients/gobpfd/v1"
	v1 "k8s.io/api/core/v1"

	//+kubebuilder:scaffold:imports

	"google.golang.org/grpc"
	//"google.golang.org/grpc/credentials/insecure"
)

var (
	scheme   = runtime.NewScheme()
	setupLog = ctrl.Log.WithName("setup")
)

func init() {
	utilruntime.Must(clientgoscheme.AddToScheme(scheme))
	utilruntime.Must(bpfdiov1alpha1.AddToScheme(scheme))
	utilruntime.Must(v1.AddToScheme(scheme))
	//+kubebuilder:scaffold:scheme
}

func main() {
	var metricsAddr string
	var probeAddr string
	var opts zap.Options
	flag.StringVar(&metricsAddr, "metrics-bind-address", ":8080", "The address the metric endpoint binds to.")
	flag.StringVar(&probeAddr, "health-probe-bind-address", ":8081", "The address the probe endpoint binds to.")
	flag.Parse()

	// Get the Log level for bpfd deployment where this pod is running
	logLevel := os.Getenv("GO_LOG")
	switch logLevel {
	case "info":
		opts = zap.Options{
			Development: false,
		}
	case "debug":
		opts = zap.Options{
			Development: true,
		}
	default:
		opts = zap.Options{
			Development: false,
		}
	}

	ctrl.SetLogger(zap.New(zap.UseFlagOptions(&opts)))

	mgr, err := ctrl.NewManager(ctrl.GetConfigOrDie(), ctrl.Options{
		Scheme:                 scheme,
		MetricsBindAddress:     metricsAddr,
		Port:                   9443,
		HealthProbeBindAddress: probeAddr,
		LeaderElection:         false,
		// Specify that Secrets's should not be cached.
		ClientDisableCacheFor: []client.Object{&v1.Secret{}},
	})
	if err != nil {
		setupLog.Error(err, "unable to start manager")
		os.Exit(1)
	}

	// Setup bpfd Client
	configFileData := tls.LoadConfig()

	creds, err := tls.LoadTLSCredentials(configFileData.Tls)
	if err != nil {
		setupLog.Error(err, "Failed to generate credentials for new client")
		os.Exit(1)
	}

	// Set up a connection to bpfd, block until bpfd is up.
	addr := fmt.Sprintf("localhost:%d", configFileData.Grpc.Endpoint.Port)
	setupLog.WithValues("addr", addr).WithValues("creds", creds).Info("Waiting for active connection to bpfd at %s")
	conn, err := grpc.DialContext(context.Background(), addr, grpc.WithTransportCredentials(creds), grpc.WithBlock())
	if err != nil {
		setupLog.Error(err, "unable to connect to bpfd")
		os.Exit(1)
	}

	// TODO(ASTOYCOS) add support for connecting over unix sockets.
	// Set up a connection to bpfd, block until bpfd is up.
	// addr := "unix:/var/lib/bpfd/bpfd.sock"
	// setupLog.WithValues("addr", addr).Info("Waiting for active connection to bpfd at %s")
	// conn, err := grpc.DialContext(context.Background(), addr, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithBlock())
	// if err != nil {
	// 	setupLog.Error(err, "unable to connect to bpfd")
	// 	os.Exit(1)
	// }

	// Get the nodename where this pod is running
	nodeName := os.Getenv("NODENAME")
	if nodeName == "" {
		setupLog.Error(fmt.Errorf("NODENAME env var not set"), "Couldn't determine bpfd-agent's node")
		os.Exit(1)
	}

	// Get the bpfd deployments Namespace
	namespace := os.Getenv("NAMESPACE")
	if namespace == "" {
		setupLog.Error(fmt.Errorf("NAMESPACE env var not set"), "Couldn't determine bpfd-agent's namespace")
		os.Exit(1)
	}

	common := bpfdagent.ReconcilerCommon{
		Client:     mgr.GetClient(),
		Scheme:     mgr.GetScheme(),
		GrpcConn:   conn,
		BpfdClient: gobpfd.NewLoaderClient(conn),
		Namespace:  namespace,
		NodeName:   nodeName,
	}

	if err = (&bpfdagent.XdpProgramReconciler{
		ReconcilerCommon: common,
	}).SetupWithManager(mgr); err != nil {
		setupLog.Error(err, "unable to create xdpProgram controller", "controller", "BpfProgram")
		os.Exit(1)
	}

	if err = (&bpfdagent.TcProgramReconciler{
		ReconcilerCommon: common,
	}).SetupWithManager(mgr); err != nil {
		setupLog.Error(err, "unable to create tcProgram controller", "controller", "BpfProgram")
		os.Exit(1)
	}

	if err = (&bpfdagent.TracePointProgramReconciler{
		ReconcilerCommon: common,
	}).SetupWithManager(mgr); err != nil {
		setupLog.Error(err, "unable to create tracepointProgram controller", "controller", "BpfProgram")
		os.Exit(1)
	}

	//+kubebuilder:scaffold:builder

	if err := mgr.AddHealthzCheck("healthz", healthz.Ping); err != nil {
		setupLog.Error(err, "unable to set up health check")
		os.Exit(1)
	}
	if err := mgr.AddReadyzCheck("readyz", healthz.Ping); err != nil {
		setupLog.Error(err, "unable to set up ready check")
		os.Exit(1)
	}

	setupLog.Info("starting Bpfd-Agent")
	if err := mgr.Start(ctrl.SetupSignalHandler()); err != nil {
		setupLog.Error(err, "problem running manager")
		os.Exit(1)
	}
}
